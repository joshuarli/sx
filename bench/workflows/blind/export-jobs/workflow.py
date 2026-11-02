import json
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

REPORTS = ("summary", "detail", "audit", "ledger")
PARTS = {"summary": 3, "detail": 2, "ledger": 2}
ROWS = {"summary": 14, "detail": 9, "ledger": 11}
JOBS = {
    "job-1": ("summary", ["queued", "running", "ready"]),
    "job-2": ("detail", ["queued", "failed"]),
    "job-3": ("detail", ["running", "ready"]),
    "job-4": ("audit", ["queued", "running"]),
    "job-5": ("ledger", ["ready"]),
}
CREATED = ["summary", "detail", "detail", "audit", "audit", "ledger"]
REFUSED = {"audit": 1}
UNAVAILABLE_PARTS = {("job-3", 2): 1}
WAITS = (0.05, 0.1, 0.2, 0.2, 0.2, 0.2, 0.2, 0.2)
SLACK = 0.004
STALE = "stale ledger export\n"


def content(report: str) -> str:
    return "".join(f"{report},{row:04d},{row * 37 % 1000}\n" for row in range(ROWS[report]))


def pieces(report: str) -> list[str]:
    text = content(report)
    size = -(-len(text) // PARTS[report])
    return [text[start : start + size] for start in range(0, len(text), size)]


def claimed_bytes(report: str) -> int:
    return len(content(report)) + (2 if report == "ledger" else 0)


def expected() -> tuple[dict[str, object], dict[str, str]]:
    rows: list[dict[str, object]] = []
    files: dict[str, str] = {"ledger.csv": STALE}
    outcomes = {
        "summary": ("written", 1, PARTS["summary"]),
        "detail": ("written", 2, PARTS["detail"]),
        "audit": ("timeout", 1, 0),
        "ledger": ("corrupt", 1, PARTS["ledger"]),
    }
    for report in REPORTS:
        outcome, jobs, parts = outcomes[report]
        written = len(content(report)) if outcome == "written" else 0
        if outcome == "written":
            files[f"{report}.csv"] = content(report)
        rows.append({"report": report, "outcome": outcome, "jobs": jobs, "parts": parts, "bytes": written})
    return {"reports": rows, "written": 2}, files


def setup(sandbox: Sandbox) -> None:
    sandbox.write("exports/ledger.csv", STALE)
    guard = threading.Lock()
    created: list[str] = []
    refused: dict[str, int] = defaultdict(int)
    polls: dict[str, int] = defaultdict(int)
    flaky: dict[tuple[str, int], int] = defaultdict(int)

    def handle(request: Request) -> Response:
        parts = [part for part in request.path.split("/") if part]
        with guard:
            if request.method == "POST" and parts == ["exports"]:
                report = json.loads(request.body)["report"]
                if report not in REPORTS:
                    return Response.json('{"error": "unknown report"}', 404)
                if refused[report] < REFUSED.get(report, 0):
                    refused[report] += 1
                    return Response.json('{"error": "slow down"}', 429)
                created.append(report)
                return Response.json(json.dumps({"id": f"job-{len(created)}"}), 202)
            if request.method == "GET" and len(parts) >= 2 and parts[0] == "exports" and parts[1] in JOBS:
                job = parts[1]
                report, script = JOBS[job]
                if len(created) < int(job.removeprefix("job-")) or created[int(job.removeprefix("job-")) - 1] != report:
                    return Response.json('{"error": "no such job"}', 404)
                if len(parts) == 2:
                    state = script[min(polls[job], len(script) - 1)]
                    polls[job] += 1
                    body: dict[str, object] = {"status": state, "parts": None, "bytes": None}
                    if state == "ready":
                        body.update({"parts": PARTS[report], "bytes": claimed_bytes(report)})
                    return Response.json(json.dumps(body))
                if len(parts) == 4 and parts[2] == "parts" and script[-1] == "ready":
                    number = int(parts[3])
                    if not 1 <= number <= PARTS[report]:
                        return Response.json('{"error": "no such part"}', 404)
                    if flaky[(job, number)] < UNAVAILABLE_PARTS.get((job, number), 0):
                        flaky[(job, number)] += 1
                        return Response.json('{"error": "try later"}', 503)
                    return Response.json(json.dumps({"n": number, "text": pieces(report)[number - 1]}))
        return Response.json('{"error": "not found"}', 404)

    sandbox.serve("reports", handle, latency=0.02)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, files = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    directory = sandbox.root / "exports"
    found = {path.name: path.read_text() for path in directory.iterdir()}
    if found != files:
        differing = sorted(name for name in found if found[name] != files.get(name))
        return Verdict.wrong(f"the exports directory holds {sorted(found)}, with differing text: {differing}")
    posts: list[str] = []
    status: dict[str, list[float]] = defaultdict(list)
    downloads: dict[str, list[int]] = defaultdict(list)
    post_times: dict[str, list[float]] = defaultdict(list)
    part_times: dict[tuple[str, int], list[float]] = defaultdict(list)
    for request in sandbox.services["reports"].requests:
        parts = [part for part in request.path.split("/") if part]
        if request.method == "POST":
            report = json.loads(request.body)["report"]
            posts.append(report)
            post_times[report].append(request.received)
        elif len(parts) == 2:
            status[parts[1]].append(request.received)
        else:
            downloads[parts[1]].append(int(parts[3]))
            part_times[(parts[1], int(parts[3]))].append(request.received)
    if posts != CREATED:
        return Verdict.wrong(f"the jobs were created in the order {posts}, not {CREATED}")
    if any(b - a < 0.1 - SLACK for a, b in pairwise(post_times["audit"])):
        return Verdict.wrong("the refused job creation was repeated too soon")
    for job, (_, script) in JOBS.items():
        needed = len(script) if script[-1] != "running" else len(WAITS) + 1
        if len(status[job]) != needed:
            return Verdict.wrong(f"{job} was asked about {len(status[job])} times, not {needed}")
        for gap, wait in zip((b - a for a, b in pairwise(status[job])), WAITS, strict=False):
            if gap < wait - SLACK:
                return Verdict.wrong(f"{job} was asked about again after {gap * 1000:.0f} ms, not {wait * 1000:.0f}")
    wanted_downloads = {"job-1": [1, 2, 3], "job-3": [1, 2, 2], "job-5": [1, 2]}
    if {job: sorted(numbers) for job, numbers in downloads.items()} != wanted_downloads:
        return Verdict.wrong(f"the parts were downloaded as {dict(downloads)}")
    for (job, number), times in part_times.items():
        if len(times) == 2 and times[1] - times[0] < 0.05 - SLACK:
            return Verdict.wrong(f"part {number} of {job} was asked for again too soon")
    if sandbox.services["reports"].peak > 2:
        return Verdict.wrong(f"{sandbox.services['reports'].peak} requests were in flight at once, more than 2")
    return Verdict.correct()
