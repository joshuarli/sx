import json
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

TABLE = (("api", "1.4.0"), ("web", "2.1.0"), ("worker", "0.9.3"), ("billing", "3.0.0"), ("search", "1.1.1"))
POLLS_TO_FINISH = {"api": ("done", 2), "web": ("done", 3), "worker": ("failed", 3), "billing": ("failed", 2)}
REFUSED_STARTS = {"worker": 1}
INTERVAL = 0.1
DEADLINE = 1.0
SLACK = 0.01
MIN_STUCK_POLLS = 7
MAX_STUCK_POLLS = 12
OUTCOMES = {"api": "done", "web": "done", "worker": "rolled-back", "billing": "rolled-back", "search": "timeout"}


def identifier(service: str) -> str:
    return f"d-{service}"


def expected() -> object:
    deployments = [{"service": name, "id": identifier(name), "outcome": OUTCOMES[name]} for name, _ in TABLE]
    outcomes = list(OUTCOMES.values())
    return {
        "deployments": deployments,
        "done": outcomes.count("done"),
        "rolled_back": outcomes.count("rolled-back"),
        "timed_out": outcomes.count("timeout"),
    }


class Deployments:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.started: dict[str, str] = {}
        self.refused: dict[str, int] = defaultdict(int)
        self.polls: dict[str, int] = defaultdict(int)
        self.rolled_back: set[str] = set()

    def handle(self, request: Request) -> Response:
        parts = [part for part in request.path.split("/") if part]
        with self.lock:
            if request.method == "POST" and parts == ["deployments"]:
                return self.start(request)
            if request.method == "GET" and len(parts) == 2 and parts[0] == "deployments":
                return self.status(parts[1])
            if request.method == "POST" and len(parts) == 3 and parts[0] == "deployments" and parts[2] == "rollback":
                return self.rollback(parts[1])
        return Response.json('{"error": "not found"}', 404)

    def start(self, request: Request) -> Response:
        body = json.loads(request.body or b"{}")
        service = str(body.get("service"))
        known = dict(TABLE)
        if service not in known or body.get("version") != known[service]:
            return Response.json('{"error": "unknown service or version"}', 400)
        if service in self.started:
            return Response.json('{"error": "already deploying"}', 409)
        if self.refused[service] < REFUSED_STARTS.get(service, 0):
            self.refused[service] += 1
            return Response.json('{"error": "rate limited"}', 429)
        self.started[service] = identifier(service)
        return Response.json(json.dumps({"id": identifier(service), "status": "running"}), 201)

    def status(self, deployment: str) -> Response:
        service = deployment.removeprefix("d-")
        if self.started.get(service) != deployment:
            return Response.json('{"error": "no such deployment"}', 404)
        self.polls[deployment] += 1
        final, needed = POLLS_TO_FINISH.get(service, ("running", 0))
        status = final if needed and self.polls[deployment] >= needed else "running"
        if deployment in self.rolled_back:
            status = "rolled-back"
        return Response.json(json.dumps({"id": deployment, "service": service, "status": status}))

    def rollback(self, deployment: str) -> Response:
        service = deployment.removeprefix("d-")
        final, needed = POLLS_TO_FINISH.get(service, ("running", 0))
        if final != "failed" or self.polls[deployment] < needed or deployment in self.rolled_back:
            return Response.json('{"error": "only a failed deployment can be rolled back"}', 409)
        self.rolled_back.add(deployment)
        return Response.json(json.dumps({"id": deployment, "status": "rolled-back"}))


def setup(sandbox: Sandbox) -> None:
    sandbox.serve("deploy", Deployments().handle)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    starts: dict[str, list[float]] = defaultdict(list)
    polls: dict[str, list[float]] = defaultdict(list)
    rollbacks: dict[str, list[float]] = defaultdict(list)
    for request in sandbox.services["deploy"].requests:
        parts = [part for part in request.path.split("/") if part]
        if request.method == "POST" and parts == ["deployments"]:
            starts[str(json.loads(request.body)["service"])].append(request.received)
        elif request.method == "GET" and len(parts) == 2:
            polls[parts[1]].append(request.received)
        elif request.method == "POST" and len(parts) == 3:
            rollbacks[parts[1]].append(request.received)
        else:
            return Verdict.wrong(f"unexpected request {request.method} {request.path}")
    if sorted(starts) != sorted(name for name, _ in TABLE):
        return Verdict.wrong(f"starts were requested for {sorted(starts)}")
    for service, _ in TABLE:
        wanted_starts = 1 + REFUSED_STARTS.get(service, 0)
        if len(starts[service]) != wanted_starts:
            return Verdict.wrong(f"{service} was started {len(starts[service])} times, not {wanted_starts}")
        for before, after in pairwise(starts[service]):
            if after - before < INTERVAL - SLACK:
                return Verdict.wrong(f"the refused start of {service} was repeated after {(after - before) * 1000:.0f} ms")
        if not polls[identifier(service)] or polls[identifier(service)][0] < starts[service][-1]:
            return Verdict.wrong(f"{service} was not polled after its start")
    for service, (_, needed) in POLLS_TO_FINISH.items():
        count = len(polls[identifier(service)])
        if count != needed:
            return Verdict.wrong(f"{service} was polled {count} times; it finishes at poll {needed} and polling stops there")
    stuck = polls[identifier("search")]
    if not MIN_STUCK_POLLS <= len(stuck) <= MAX_STUCK_POLLS:
        return Verdict.wrong(f"search was polled {len(stuck)} times, outside {MIN_STUCK_POLLS} to {MAX_STUCK_POLLS}")
    if not DEADLINE - 0.2 <= stuck[-1] - stuck[0] <= DEADLINE + 0.25:
        return Verdict.wrong(f"search was polled over {stuck[-1] - stuck[0]:.2f} s, not about {DEADLINE} s")
    for deployment, times in polls.items():
        for before, after in pairwise(times):
            if after - before < INTERVAL - SLACK:
                return Verdict.wrong(f"{deployment} was polled again after {(after - before) * 1000:.0f} ms")
    failed = [identifier("billing"), identifier("worker")]
    if sorted(rollbacks) != failed or any(len(times) != 1 for times in rollbacks.values()):
        return Verdict.wrong(f"rollbacks were requested for { {k: len(v) for k, v in rollbacks.items()} }")
    for deployment in failed:
        if rollbacks[deployment][0] < polls[deployment][-1]:
            return Verdict.wrong(f"{deployment} was rolled back before it was seen failing")
    return Verdict.correct()
