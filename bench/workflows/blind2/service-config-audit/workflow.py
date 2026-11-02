import json
import re
from collections import Counter

from definition import Evidence, Verdict
from sandbox import Sandbox

SOURCE = "https://git.example.com"


def config(**fields: object) -> str:
    return json.dumps(fields, indent=2) + "\n"


FILES: dict[str, str] = {
    "README.md": "Service configuration. Edit with care.\n",
    "api/service.json": config(
        name="api", version="2.3.1", port=3000, owner="team-core", repository=f"{SOURCE}/api.git"
    ),
    "api/examples/service.json": config(name="example", version="x", port=1),
    "billing/service.json": config(
        name="billing", version="3.0.0", port=8080, owner="team-pay", repository=f"{SOURCE}/billing.git"
    ),
    "web/service.json": config(
        name="web",
        version="2.1.0",
        port=8080,
        owner="team-web",
        repository={"url": f"{SOURCE}/web.git", "branch": "main"},
    ),
    "worker/service.json": config(
        name="worker", version="0.9.3", port="9000", owner="", repository="git@git.example.com:worker.git"
    ),
    "search/service.json": '{\n  "name": "search",\n  "version": "1.1.1",\n}\n',
    "empty/service.json": "",
    "legacy/service.json": config(name="legacy", version="0.1.0", port=80, repository=f"{SOURCE}/legacy.git"),
    "reports/service.json": config(
        name="reports", version="1.4.0", port=9100, owner="team-data", repository=f"{SOURCE}/reports.git"
    ),
    "jobs/service.json": config(
        name="jobs", version="1.0", port=9100, owner="team-data", repository={"url": f"{SOURCE}/jobs.git"}
    ),
    "docs/README.md": "Documentation sources, not a service.\n",
    "_template/service.json": config(name="template", version="0.0.0", port=1024),
    ".cache/service.json": "not even json",
    "platform/notes.txt": "platform group\n",
    "platform/auth/service.json": config(
        name="auth", version="1.8.2", port=4100, owner="team-id", repository=f"{SOURCE}/auth.git"
    ),
    "platform/gateway/service.json": config(
        name="gateway", version="1.2", port=4101, owner="team-edge", repository=f"{SOURCE}/gateway.git"
    ),
    "platform/_old/service.json": config(name="old", version="0.0.1", port=4000),
    "platform/deep/metrics/service.json": config(
        name="metric", version="1.0.0", port=4102, owner="team-obs", repository={"url": "http://git.example.com/m.git"}
    ),
    "platform/deep/metrics/agent/service.json": config(name="agent", version="1.0.0", port=4103),
    "platform/deep/too/far/service.json": config(name="far", version="1.0.0", port=4104),
}
ORDER = ("name", "version", "port", "port-conflict", "repository", "owner")


def audit() -> tuple[str, dict[str, object]]:
    roots = [path.removesuffix("/service.json") for path in FILES if path.endswith("/service.json")]
    services = sorted(
        path
        for path in roots
        if not any(part.startswith(("_", ".")) for part in path.split("/"))
        and len(path.split("/")) <= 3
        and not any(path.startswith(other + "/") for other in roots if other != path)
        and path != ""
    )
    loaded: dict[str, dict[str, object] | None] = {}
    for path in services:
        try:
            value = json.loads(FILES[f"{path}/service.json"])
        except json.JSONDecodeError:
            value = None
        loaded[path] = value

    def port_of(value: dict[str, object]) -> int | None:
        port = value.get("port")
        if isinstance(port, int) and not isinstance(port, bool) and 1024 <= port <= 65535:
            return port
        return None

    ports = Counter(port_of(v) for v in loaded.values() if v is not None)
    lines: list[str] = []
    statuses: dict[str, str] = {}
    tally: Counter[str] = Counter()
    for path in services:
        value = loaded[path]
        if value is None:
            statuses[path] = "invalid"
            lines.append(f"{path}\tinvalid\t\n")
            continue
        port = port_of(value)
        repository = value.get("repository")
        version = value.get("version")
        url = repository.get("url") if isinstance(repository, dict) else repository
        broken = {
            "name": value.get("name") != path.split("/")[-1],
            "version": not isinstance(version, str) or not re.fullmatch(r"\d+\.\d+\.\d+", version),
            "port": port is None,
            "port-conflict": port is not None and ports[port] > 1,
            "repository": not (isinstance(url, str) and url.startswith("https://")),
            "owner": not (isinstance(value.get("owner"), str) and value["owner"] != ""),
        }
        findings = [name for name in ORDER if broken[name]]
        tally.update(findings)
        statuses[path] = "fail" if findings else "ok"
        lines.append(f"{path}\t{statuses[path]}\t{','.join(findings)}\n")
    ranked = sorted(tally.items(), key=lambda item: (-item[1], item[0]))
    result: dict[str, object] = {
        "services": len(services),
        "ok": list(statuses.values()).count("ok"),
        "fail": list(statuses.values()).count("fail"),
        "invalid": list(statuses.values()).count("invalid"),
        "invalid_paths": [p for p in services if statuses[p] == "invalid"],
        "findings": [{"finding": n, "count": c} for n, c in ranked],
    }
    return "".join(lines), result


def setup(sandbox: Sandbox) -> None:
    for path, text in FILES.items():
        sandbox.write(f"services/{path}", text)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    tsv, wanted = audit()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    path = sandbox.root / "audit.tsv"
    if not path.is_file():
        return Verdict.wrong("audit.tsv was not written")
    if path.read_bytes() != tsv.encode():
        return Verdict.wrong(f"audit.tsv holds {path.read_text()!r}, not {tsv!r}")
    present = {
        str(item.relative_to(sandbox.root / "services")): item.read_text()
        for item in (sandbox.root / "services").rglob("*")
        if item.is_file()
    }
    if present != FILES:
        return Verdict.wrong("the program changed the files under services")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name not in {"services", "audit.tsv"})
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
