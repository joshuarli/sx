import json
import threading
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

HISTORY = (
    ("chore: initial import", ()),
    ("feat(core): first parser", ("v0.9.0",)),
    ("fix: crash on empty file", ("release-2020",)),
    ("feat: plugins", ("v1.2.0",)),
    ("feat(cli)!: rename flags", ("v1.9.0",)),
    ("fix(cli): flag parsing", ("v1.10.0",)),
    ("fix: late hotfix for the 1.9 line", ("v1.9.1",)),
    ("feat(api): add a release candidate endpoint", ("v1.10.1-rc.1", "v2.0.0-beta.1", "nightly")),
    ("fix(parser): handle empty input", ()),
    ("docs: explain flags", ()),
    ("feat(cli): add --json output", ()),
    ("Merge branch 'hotfix'", ()),
    ("fix(io): reject names with colons: not allowed", ()),
    ("chore(deps): bump regex", ()),
    ("perf(index): cache lookups", ()),
    ("feature: not a conventional type", ()),
    ("Feat: wrongly capitalised", ()),
)
LATEST = "v1.10.0"
NEW_TAG = "v1.11.0"
SINCE = 11
NOTES = "\n".join(
    [
        "- cache lookups",
        "- reject names with colons: not allowed",
        "- add --json output",
        "- handle empty input",
        "- add a release candidate endpoint",
        "- late hotfix for the 1.9 line",
    ]
)
RELEASE_ID = 7001
DELAY = 0.1
SLACK = 0.005
ORIGINAL_TAGS = sorted(tag for _, tags in HISTORY for tag in tags)


def setup(sandbox: Sandbox) -> None:
    path = sandbox.directory("repo")
    sandbox.git(path, "init", "--quiet")
    for subject, tags in HISTORY:
        sandbox.git(path, "commit", "--quiet", "--allow-empty", "--message", subject)
        for tag in tags:
            sandbox.git(path, "tag", tag)
    lock = threading.Lock()
    state = {"posts": 0, "published": 0}

    def handle(request: Request) -> Response:
        if request.method != "POST" or request.path != "/releases":
            return Response.json('{"error": "not found"}', 404)
        body = json.loads(request.body or b"{}")
        with lock:
            state["posts"] += 1
            if state["posts"] == 1:
                return Response.json('{"error": "rate limited"}', 429)
            tags = sandbox.git(path, "tag", "--list").split()
            if body.get("tag") not in tags:
                return Response.json('{"error": "no such tag in the repository"}', 409)
            if state["published"]:
                return Response.json('{"error": "already published"}', 409)
            state["published"] += 1
            return Response.json(json.dumps({"id": RELEASE_ID, "tag": body["tag"]}), 201)

    sandbox.serve("release", handle)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    path = sandbox.root / "repo"
    head = sandbox.git(path, "rev-parse", "HEAD").strip()
    wanted = {
        "latest": LATEST,
        "since": SINCE,
        "bump": "minor",
        "tag": NEW_TAG,
        "commit": head,
        "release_id": RELEASE_ID,
        "notes": NOTES,
    }
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    tags = sorted(sandbox.git(path, "tag", "--list").split())
    if tags != sorted([*ORIGINAL_TAGS, NEW_TAG]):
        return Verdict.wrong(f"the repository holds the tags {tags}")
    if sandbox.git(path, "rev-parse", NEW_TAG).strip() != head:
        return Verdict.wrong(f"{NEW_TAG} does not point at HEAD")
    subjects = sandbox.git(path, "log", "--format=%s").splitlines()
    if subjects != [subject for subject, _ in reversed(HISTORY)] or sandbox.git(path, "status", "--porcelain"):
        return Verdict.wrong("the repository was changed beyond the new tag")
    posts = list(sandbox.services["release"].requests)
    if len(posts) != 2 or any(r.method != "POST" for r in posts):
        return Verdict.wrong(f"the service saw {[(r.method, r.path) for r in posts]}, not two publishes")
    for request in posts:
        if json.loads(request.body) != {"tag": NEW_TAG, "notes": NOTES}:
            return Verdict.wrong(f"a publish carried {request.body!r}")
    for before, after in pairwise(r.received for r in posts):
        if after - before < DELAY - SLACK:
            return Verdict.wrong(f"the refused publish was repeated after {(after - before) * 1000:.0f} ms")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "repo")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
