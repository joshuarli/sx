import json
from collections import Counter

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

REGISTRY = {
    "left-pad": ["1.9.0", "1.0.0", "2.0.0-beta.1", "1.10.0", "1.3.0", "1.1.0"],
    "http-kit": ["3.1.9", "2.4.1", "3.1.10", "2.5.0", "3.0.0", "3.1.2", "3.2.0-rc.1"],
    "tiny-log": ["0.10.1", "0.9.0", "0.10.0"],
    "css-tools": ["4.0.0", "3.9.9", "3.0.0", "4.0.1-alpha"],
    "db-driver": ["5.2.0", "4.9.9", "5.1.0", "5.10.0b1", "5.2"],
    "json-schema-lite": ["1.9.0", "1.8.0", "2.0.0"],
    "queue-core": ["0.4.0", "0.3.7", "0.3.0"],
    "retry-it": ["1.2.5", "2.0.0-rc.1", "1.0.0", "1.2.4"],
    "arg-parse": ["3.2.4", "3.2.1", "3.2.3", "3.2.10"],
}
MANIFESTS = {
    "web": "# web frontend\nleft-pad==1.3.0\nhttp-kit == 2.5.0\n\ntiny-log==0.10.0\ncss-tools==4.0.0\nghost-pkg==1.0.0\n",
    "api": "http-kit==3.1.9\ndb-driver==5.2.0\n# pinned\nleft-pad==1.10.0\njson-schema-lite == 2.0.0\nqueue-core==0.3.7\n",
    "worker": "db-driver==4.9.9\nqueue-core==0.3.7\n\ntiny-log==0.10.1\nretry-it==1.0.0\nghost-pkg==2.0.0\n",
    "cli": "arg-parse==3.2.1\ntiny-log==0.9.0\ncss-tools==3.9.9\n",
}
LATENCY = 0.05
LIMIT = 4
MIN_PEAK = 2
KINDS = ("major", "minor", "patch")
type Triple = tuple[int, int, int]


def triple(text: str) -> Triple | None:
    parts = text.split(".")
    if len(parts) != 3 or not all(part.isascii() and part.isdigit() and str(int(part)) == part for part in parts):
        return None
    return int(parts[0]), int(parts[1]), int(parts[2])


def latest(name: str) -> Triple | None:
    versions = [parsed for text in REGISTRY[name] if (parsed := triple(text)) is not None]
    return max(versions) if versions else None


def show(value: Triple) -> str:
    return ".".join(str(part) for part in value)


def dependencies() -> list[tuple[str, str, str]]:
    found = []
    for project, text in sorted(MANIFESTS.items()):
        for line in text.split("\n"):
            if line.strip() and not line.startswith("#"):
                name, version = (part.strip() for part in line.split("=="))
                found.append((project, name, version))
    return sorted(found)


def expected() -> object:
    outdated = []
    missing = []
    for project, name, version in dependencies():
        if name not in REGISTRY:
            missing.append({"project": project, "package": name})
            continue
        best, current = latest(name), triple(version)
        assert best is not None and current is not None
        if best > current:
            kind = next(KINDS[index] for index in range(3) if best[index] != current[index])
            outdated.append(
                {"project": project, "package": name, "current": version, "latest": show(best), "kind": kind}
            )
    counts = Counter(str(item["kind"]) for item in outdated)
    return {"outdated": outdated, "missing": missing, "counts": {kind: counts[kind] for kind in KINDS}}


def setup(sandbox: Sandbox) -> None:
    def registry(request: Request) -> Response:
        parts = request.path.split("?")[0].split("/")
        if request.method != "GET" or len(parts) != 3 or parts[1] != "packages":
            return Response.json('{"error": "not found"}', 404)
        if parts[2] not in REGISTRY:
            return Response.json('{"error": "package not found"}', 404)
        return Response.json(json.dumps({"name": parts[2], "versions": REGISTRY[parts[2]]}))

    sandbox.serve("registry", registry, latency=LATENCY)
    for project, text in MANIFESTS.items():
        sandbox.write(f"manifests/{project}.deps", text)
    sandbox.write("manifests/NOTES.md", "ghost-pkg==9.9.9\nunlisted==1.0.0\n")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    service = sandbox.services["registry"]
    asked = sorted(request.path for request in service.requests)
    names = sorted({name for _, name, _ in dependencies()})
    wanted_paths = [f"/packages/{name}" for name in names]
    if asked != wanted_paths:
        return Verdict.wrong(f"the registry was asked for {asked}, not once for each of {wanted_paths}")
    if service.peak > LIMIT:
        return Verdict.wrong(f"{service.peak} registry requests were in flight at once; the limit is {LIMIT}")
    if service.peak < MIN_PEAK:
        return Verdict.wrong("the registry requests were not run concurrently")
    names_left = sorted(path.name for path in sandbox.root.iterdir())
    if names_left != ["manifests"]:
        return Verdict.wrong(f"the program left {names_left} in the sandbox")
    return Verdict.correct()
