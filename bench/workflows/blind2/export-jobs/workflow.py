import json
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

REPORTS = ("sales", "refunds", "payouts", "adjustments")
SCRIPTS: dict[str, list[tuple[str, int, str | None]]] = {
    "sales": [("done", 4, None)],
    "refunds": [("failed", 2, "transient"), ("done", 3, None)],
    "payouts": [("stuck", 0, None)],
    "adjustments": [("failed", 2, "bad-month")],
}
FILES = {
    "sales": ["day,total", "2026-03-01,1200", "2026-03-02,980", "2026-03-03,1105"],
    "refunds": ["order,amount", "o-31,45", "o-77,120"],
}
REFUSED_FIRST = "refunds"
WAITS = (0.05, 0.1, 0.2, 0.2, 0.2, 0.2)
CREATE_WAIT = 0.05
SLACK = 0.005
STUCK_POLLS = 7
MONTH = "2026-03"


def expected() -> tuple[dict[str, object], list[tuple[str, str]], dict[str, list[str]]]:
    sequence: list[tuple[str, str]] = []
    reports: list[dict[str, object]] = []
    counter = 0
    for report in REPORTS:
        jobs: list[str] = []
        outcome, rows = "failed", None
        for kind, polls, reason in SCRIPTS[report]:
            if report == REFUSED_FIRST and not jobs:
                sequence.append(("POST", "/exports"))
            sequence.append(("POST", "/exports"))
            counter += 1
            job = f"job-{counter}"
            jobs.append(job)
            count = STUCK_POLLS if kind == "stuck" else polls
            sequence += [("GET", f"/exports/{job}")] * count
            if kind == "stuck":
                outcome = "timeout"
            elif kind == "done":
                sequence.append(("GET", f"/exports/{job}/file"))
                outcome, rows = "exported", len(FILES[report])
            elif reason != "transient":
                outcome = "failed"
        reports.append({"report": report, "outcome": outcome, "jobs": jobs, "rows": rows})
    return {"reports": reports}, sequence, dict(FILES)


def setup(sandbox: Sandbox) -> None:
    lock = threading.Lock()
    created: dict[str, int] = defaultdict(int)
    refused = {"done": False}
    jobs: dict[str, tuple[str, int]] = {}
    polls: dict[str, int] = defaultdict(int)
    counter = {"next": 1}

    def handle(request: Request) -> Response:
        parts = [part for part in request.path.split("/") if part]
        with lock:
            if request.method == "POST" and parts == ["exports"]:
                body = json.loads(request.body or b"{}")
                report = str(body.get("report"))
                if report not in SCRIPTS or body.get("month") != MONTH:
                    return Response.json('{"error": "unknown report"}', 400)
                if report == REFUSED_FIRST and not refused["done"]:
                    refused["done"] = True
                    return Response.json('{"error": "rate limited"}', 429)
                if created[report] >= len(SCRIPTS[report]):
                    return Response.json('{"error": "too many jobs"}', 409)
                identifier = f"job-{counter['next']}"
                counter["next"] += 1
                jobs[identifier] = (report, created[report])
                created[report] += 1
                return Response.json(json.dumps({"id": identifier}), 201)
            if request.method == "GET" and len(parts) >= 2 and parts[0] == "exports" and parts[1] in jobs:
                report, index = jobs[parts[1]]
                kind, needed, reason = SCRIPTS[report][index]
                if len(parts) == 3 and parts[2] == "file":
                    if kind != "done" or polls[parts[1]] < needed:
                        return Response.json('{"error": "not ready"}', 409)
                    return Response.json(json.dumps({"lines": FILES[report]}))
                if len(parts) == 2:
                    polls[parts[1]] += 1
                    if kind == "stuck" or polls[parts[1]] < needed:
                        state = "queued" if polls[parts[1]] == 1 else "running"
                        return Response.json(json.dumps({"state": state, "reason": None}))
                    return Response.json(json.dumps({"state": kind, "reason": reason}))
        return Response.json('{"error": "not found"}', 404)

    sandbox.directory("exports")
    sandbox.serve("reports", handle)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, sequence, files = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    seen = [(r.method, r.path) for r in sandbox.services["reports"].requests]
    if seen != sequence:
        return Verdict.wrong(f"the requests were {seen}, not {sequence}")
    requests = sandbox.services["reports"].requests
    for request in requests:
        if request.method == "POST" and json.loads(request.body).get("month") != MONTH:
            return Verdict.wrong("a job was started for another month")
    refunds = [r for r in requests if r.method == "POST" and json.loads(r.body)["report"] == REFUSED_FIRST]
    if refunds[1].received - refunds[0].received < CREATE_WAIT - SLACK:
        gap = (refunds[1].received - refunds[0].received) * 1000
        return Verdict.wrong(f"the refused start was repeated after {gap:.0f} ms")
    by_job: dict[str, list[float]] = defaultdict(list)
    for request in requests:
        parts = request.path.split("/")
        if request.method == "GET" and len(parts) == 3:
            by_job[parts[2]].append(request.received)
    for job, times in by_job.items():
        for index, gap in enumerate(b - a for a, b in pairwise(times)):
            if gap < WAITS[index] - SLACK:
                return Verdict.wrong(f"{job} was asked about again after {gap * 1000:.0f} ms, wait {index + 1}")
    exports = sandbox.root / "exports"
    found = {item.name: item.read_bytes() for item in exports.iterdir()}
    wanted_files = {f"{name}.csv": ("\n".join(lines) + "\n").encode() for name, lines in files.items()}
    if found != wanted_files:
        return Verdict.wrong(f"the exports directory holds {sorted(found)} with other contents than expected")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "exports")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
