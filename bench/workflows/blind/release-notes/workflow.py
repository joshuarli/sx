import json
import re

from definition import Evidence, Verdict
from sandbox import Sandbox

HISTORY: list[tuple[str, str | None]] = [
    ("chore: import the sources", "v1.2.0"),
    ("feat(api): add cursor pagination", None),
    ("fix: handle an empty body", "v1.9.0"),
    ("feat(cli)!: drop the --legacy flag", None),
    ("docs: describe the release process", "v1.10.0"),
    ("feat(api): add rate limit headers", None),
    ("fix(parser): reject duplicate keys", None),
    ("docs: explain retries", "v1.11.0-rc.1"),
    ("perf(cache)!: switch to a sharded map", None),
    ("Merge pull request #12 from team/hotfix", None),
    ("fix!: reject negative limits", "nightly"),
    ("feat: add the export command", None),
    ("feat(auth)!: require tokens", None),
    ("chore(deps): bump regex", None),
    ("Fix: a capitalized type", None),
    ("perf(io): batch small writes", None),
    ("chore!: remove the vendored copy", None),
    ("feat(api-v2): add webhooks (beta): early access", None),
    ("fix(ui): align the footer", None),
]
RELEASE = "v1.10.0"


def setup(sandbox: Sandbox) -> None:
    repository = sandbox.directory("repo")
    sandbox.git(repository, "init", "--quiet")
    for number, (subject, tag) in enumerate(HISTORY, start=1):
        sandbox.write(f"repo/file-{number}.txt", f"{number}\n")
        sandbox.git(repository, "add", "--all")
        sandbox.git(repository, "commit", "--quiet", "--message", subject)
        if tag is not None:
            sandbox.git(repository, "tag", tag)
    sandbox.directory("out")


def expected(sandbox: Sandbox) -> tuple[str, dict[str, object]]:
    repository = sandbox.root / "repo"
    lines = sandbox.git(repository, "log", "--format=%H %s", f"{RELEASE}..HEAD").splitlines()
    sections: dict[str, list[str]] = {"Breaking changes": [], "Features": [], "Fixes": [], "Performance": []}
    pattern = re.compile(r"^([0-9a-f]{7})[0-9a-f]* ([a-z]+)(?:\(([^()]*)\))?(!)?: (.*)$")
    titles = {"feat": "Features", "fix": "Fixes", "perf": "Performance"}
    for line in lines:
        found = pattern.match(line)
        if found is None:
            continue
        short, kind, scope, bang, text = found.groups()
        entry = f"- **{scope}:** {text} ({short})" if scope else f"- {text} ({short})"
        if bang:
            sections["Breaking changes"].append(entry)
        elif kind in titles:
            sections[titles[kind]].append(entry)
    written = {title: entries for title, entries in sections.items() if entries}
    text = f"## Changes since {RELEASE}"
    for title, entries in written.items():
        text += f"\n\n### {title}\n" + "\n".join(entries)
    listed = sum(len(entries) for entries in written.values())
    result: dict[str, object] = {
        "tag": RELEASE,
        "commits": len(lines),
        "listed": listed,
        "skipped": len(lines) - listed,
        "sections": [{"title": title, "entries": len(entries)} for title, entries in written.items()],
    }
    return text + "\n", result


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    text, wanted = expected(sandbox)
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    notes = sandbox.root / "out" / "RELEASE_NOTES.md"
    if not notes.is_file():
        return Verdict.wrong("RELEASE_NOTES.md was not written")
    if notes.read_text() != text:
        return Verdict.wrong(f"RELEASE_NOTES.md holds {notes.read_text()!r}, not {text!r}")
    extra = sorted(path.name for path in (sandbox.root / "out").iterdir() if path.name != "RELEASE_NOTES.md")
    if extra:
        return Verdict.wrong(f"the program left {extra} in out")
    if sandbox.git(sandbox.root / "repo", "status", "--porcelain") != "":
        return Verdict.wrong("the repository was changed")
    return Verdict.correct()
