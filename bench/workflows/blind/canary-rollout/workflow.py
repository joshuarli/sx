import json
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

H, D, S = "healthy", "degraded", "starting"
SERVICES = ("checkout", "search", "billing")
STAGES = (5, 25, 50, 100)
SCRIPTS: dict[tuple[str, int], list[str]] = {
    ("checkout", 5): [H, H, H],
    ("checkout", 25): [S, H, H, D, H, H, H],
    ("checkout", 50): [H, D, H, D, H, H, H],
    ("checkout", 100): [H, H, H],
    ("search", 5): [H, H, H],
    ("search", 25): [D, H, H, H],
    ("search", 50): [H, D, D],
    ("billing", 5): [H, S] * 6,
}
REFUSALS: dict[tuple[str, int], int] = {("checkout", 50): 1, ("search", 25): 1, ("billing", 0): 2}
INTERVAL = 0.05
RETRY_DELAY = 0.1
SLACK = 0.004


def judge(states: list[str]) -> tuple[str, int]:
    good = bad = 0
    for number, state in enumerate(states, start=1):
        good = good + 1 if state == H else 0
        bad = bad + 1 if state == D else 0
        if good >= 3:
            return "passed", number
        if bad >= 2:
            return D, number
        if number == 12:
            break
    return "timeout", min(len(states), 12)


def plan() -> tuple[dict[str, object], dict[str, list[str]]]:
    services: list[dict[str, object]] = []
    sequences: dict[str, list[str]] = {}
    for name in SERVICES:
        stages: list[dict[str, object]] = []
        requests: list[str] = []
        outcome = "promoted"
        for percent in STAGES:
            requests += [f"PUT {percent}"] * (1 + REFUSALS.get((name, percent), 0))
            result, probes = judge(SCRIPTS[(name, percent)])
            requests += ["GET"] * probes
            stages.append({"percent": percent, "result": result, "probes": probes})
            if result != "passed":
                outcome = "rolled-back"
                requests += ["PUT 0"] * (1 + REFUSALS.get((name, 0), 0))
                break
        services.append({
            "service": name, "outcome": outcome, "stages": stages, "final_weight": 100 if outcome == "promoted" else 0,
        })
        sequences[name] = requests
    outcomes = [str(s["outcome"]) for s in services]
    return {
        "services": services, "promoted": outcomes.count("promoted"), "rolled_back": outcomes.count("rolled-back"),
    }, sequences


def setup(sandbox: Sandbox) -> None:
    lock = threading.Lock()
    weights: dict[str, int] = {}
    probes: dict[str, int] = defaultdict(int)
    refused: dict[tuple[str, int], int] = defaultdict(int)

    def handle(request: Request) -> Response:
        parts = request.path.split("/")
        if len(parts) != 4 or parts[1] != "services" or parts[2] not in SERVICES:
            return Response.json('{"error": "not found"}', 404)
        name = parts[2]
        with lock:
            if request.method == "PUT" and parts[3] == "weight":
                percent = int(json.loads(request.body)["percent"])
                if refused[(name, percent)] < REFUSALS.get((name, percent), 0):
                    refused[(name, percent)] += 1
                    return Response.json('{"error": "slow down"}', 429)
                weights[name] = percent
                probes[name] = 0
                return Response.json(json.dumps({"percent": percent}))
            if request.method == "GET" and parts[3] == "health":
                script = SCRIPTS.get((name, weights.get(name, 0)))
                if script is None:
                    return Response.json('{"error": "no new version at this weight"}', 409)
                state = script[min(probes[name], len(script) - 1)]
                probes[name] += 1
                return Response.json(json.dumps({"state": state}))
        return Response.json('{"error": "not found"}', 404)

    sandbox.serve("mesh", handle)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, sequences = plan()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    seen: dict[str, list[str]] = defaultdict(list)
    times: dict[str, list[tuple[str, float]]] = defaultdict(list)
    order: list[str] = []
    for request in sandbox.services["mesh"].requests:
        parts = request.path.split("/")
        name = parts[2]
        if name not in order:
            order.append(name)
        label = request.method if request.method == "GET" else f"PUT {json.loads(request.body)['percent']}"
        seen[name].append(label)
        times[name].append((label, request.received))
    if order != list(SERVICES):
        return Verdict.wrong(f"the services were handled in the order {order}")
    for name in SERVICES:
        if seen[name] != sequences[name]:
            return Verdict.wrong(f"the requests for {name} were {seen[name]}, not {sequences[name]}")
        for (before, first), (after, second) in pairwise(times[name]):
            gap = second - first
            if before == "GET" and after == "GET" and gap < INTERVAL - SLACK:
                return Verdict.wrong(f"{name} was probed again after {gap * 1000:.0f} ms")
            if before == after and before != "GET" and gap < RETRY_DELAY - SLACK:
                return Verdict.wrong(f"{name} was set again after {gap * 1000:.0f} ms")
    return Verdict.correct()
