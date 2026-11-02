import atexit
import json
import re
import shutil
import tempfile
import threading
from dataclasses import dataclass
from functools import cache
from pathlib import Path

from definition import Evidence, Verdict
from sandbox import SCRATCH, Request, Response, Sandbox, run_git

COUNT = 100
MISSING = frozenset({13, 47, 71, 88})
BROKEN = frozenset({5, 29, 64})
SHAPES = (("main", 0), ("feature/login", 1), ("fix/cache", 2), ("topic/parser", 3), ("main", 0))
CHECKS_PATH = re.compile(r"^/repos/(repo-\d{3})/commits/([0-9a-f]{40})/checks$")
PEAK_LIMIT = 8
LATENCY = 0.03
LOCK = threading.Lock()


def name(index: int) -> str:
    return f"repo-{index:03d}"


def shape(index: int) -> tuple[str, int]:
    return SHAPES[(index * 3 + index // 7) % len(SHAPES)]


def is_dirty(index: int) -> bool:
    return index % 4 == 1 or index % 9 == 4


def is_readable(index: int) -> bool:
    return index not in MISSING and index not in BROKEN


def ci_state(index: int) -> str | None:
    return ("failure", "failure", "pending", None, "success", "success", "success")[(index * 5) % 7]


@dataclass(frozen=True, slots=True)
class Template:
    directory: Path
    head: str


def templates() -> dict[tuple[str, int], Template]:
    with LOCK:
        return build_templates()


@cache
def build_templates() -> dict[tuple[str, int], Template]:
    SCRATCH.mkdir(parents=True, exist_ok=True)
    base = Path(tempfile.mkdtemp(prefix="git-ci-audit-", dir=SCRATCH)).resolve()
    atexit.register(shutil.rmtree, base, True)
    built: dict[tuple[str, int], Template] = {}
    for branch, ahead in sorted(set(SHAPES)):
        repository = base / f"{branch.replace('/', '-')}-{ahead}"
        repository.mkdir()
        run_git(repository, base, "init", "--quiet")
        for number in (1, 2):
            (repository / "README.md").write_text(f"revision {number}\n")
            run_git(repository, base, "add", "--all")
            run_git(repository, base, "commit", "--quiet", "--message", f"main {number}")
        if branch != "main":
            run_git(repository, base, "checkout", "--quiet", "-b", branch)
        for number in range(1, ahead + 1):
            (repository / f"work-{number}.txt").write_text(f"work {number}\n")
            run_git(repository, base, "add", "--all")
            run_git(repository, base, "commit", "--quiet", "--message", f"work {number}")
        built[(branch, ahead)] = Template(repository, run_git(repository, base, "rev-parse", "HEAD").strip())
    return built


def head(index: int) -> str:
    return templates()[shape(index)].head


def setup(sandbox: Sandbox) -> None:
    sandbox.directory("repos")
    sandbox.write("repos.txt", "".join(f"{name(i)}\n" for i in range(COUNT)))
    for index in range(COUNT):
        relative = f"repos/{name(index)}"
        if index in MISSING:
            continue
        if index in BROKEN:
            sandbox.write(f"{relative}/.git", "gitdir: ../absent.git\n")
            continue
        sandbox.copy_tree(templates()[shape(index)].directory, relative)
        if is_dirty(index):
            if index % 2 == 0:
                (sandbox.root / relative / "scratch.txt").write_text("untracked\n")
            else:
                (sandbox.root / relative / "README.md").write_text("edited\n")

    def checks(request: Request) -> Response:
        found = CHECKS_PATH.match(request.path)
        if request.method != "GET" or found is None:
            return Response.json('{"message": "unknown route"}', 404)
        index = int(found.group(1).removeprefix("repo-"))
        state = ci_state(index)
        if not is_readable(index) or state is None or found.group(2) != head(index):
            return Response.json('{"message": "no checks for this commit"}', 404)
        return Response.json(json.dumps({"state": state, "total": 3 + index % 4}))

    sandbox.serve("ci", checks, LATENCY)


def expected() -> dict[str, object]:
    failing, unreadable, unknown = [], [], 0
    for index in range(COUNT):
        if not is_readable(index):
            unreadable.append(name(index))
        elif not is_dirty(index):
            state = ci_state(index)
            if state is None:
                unknown += 1
            elif state == "failure":
                branch, ahead = shape(index)
                failing.append({"repo": name(index), "branch": branch, "ahead": ahead})
    failing.sort(key=lambda row: (-int(str(row["ahead"])), str(row["repo"])))
    return {"failing": failing, "unreadable": unreadable, "unknown": unknown}


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    want = expected()
    if result != want:
        return Verdict.wrong(f"the result was {json.dumps(result)[:400]}, not {json.dumps(want)[:400]}")
    paths = sorted(request.path for request in sandbox.services["ci"].requests)
    needed = sorted(
        f"/repos/{name(i)}/commits/{head(i)}/checks" for i in range(COUNT) if is_readable(i) and not is_dirty(i)
    )
    if paths != needed:
        return Verdict.wrong(
            f"CI was asked {len(paths)} times for {len(set(paths))} commits; it should be asked once for each of the "
            f"{len(needed)} clean repositories' HEAD commits and for no other"
        )
    peak = sandbox.services["ci"].peak
    if not 2 <= peak <= PEAK_LIMIT:
        return Verdict.wrong(
            f"at most {PEAK_LIMIT} CI requests may be in flight, and several should be; the peak was {peak}"
        )
    for index in range(COUNT):
        if is_readable(index):
            repository = sandbox.root / "repos" / name(index)
            if (repository / "README.md").read_text() not in ("revision 2\n", "edited\n"):
                return Verdict.wrong(f"{name(index)} was modified")
    return Verdict.correct()
