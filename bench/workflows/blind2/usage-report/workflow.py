import json
import threading
from collections import defaultdict
from itertools import pairwise
from urllib.parse import parse_qs, urlsplit

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

TOTAL = 47
PAGE_SIZE = 20
LIMIT_IN_FLIGHT = 4
DELAYS = (0.03, 0.06)
SLACK = 0.005
CLOSED = frozenset({"acc-05", "acc-19", "acc-33", "acc-40"})
UNREACHABLE = frozenset({"acc-31"})
USAGE_FAILURES: dict[str, list[int]] = {
    "acc-07": [429],
    "acc-12": [503],
    "acc-22": [429, 503],
    "acc-31": [503, 503, 503, 503],
}
PAGE_FAILURES: dict[int, list[int]] = {20: [503], 40: [429, 429]}
OVERRIDES: dict[str, tuple[int, int]] = {
    "acc-03": (400, 80),
    "acc-09": (200, 40),
    "acc-15": (1000, 200),
    "acc-21": (250, 50),
    "acc-28": (400, 80),
    "acc-36": (0, 0),
    "acc-44": (3, 1),
    "acc-45": (1000, 333),
    "acc-46": (999, 333),
}
NAMES = {"acc-03": "Acme, Inc", "acc-15": 'The "Big" Hosting Co', "acc-28": "Lumen Works"}
PLANS = ("free", "pro", "team")
type Row = dict[str, object]


def identifier(number: int) -> str:
    return f"acc-{number:02d}"


def accounts() -> list[Row]:
    return [
        {"id": identifier(n), "name": NAMES.get(identifier(n), f"Account {n:02d}"), "plan": PLANS[n % 3]}
        for n in range(1, TOTAL + 1)
    ]


def usage_of(account: str) -> tuple[int, int]:
    if account in OVERRIDES:
        return OVERRIDES[account]
    number = int(account.removeprefix("acc-"))
    requests = 100 + number * 37 % 700
    return requests, number * 13 % (requests // 12)


def expected() -> tuple[str, Row]:
    measured = [
        (row["id"], row["name"], row["plan"], *usage_of(str(row["id"])))
        for row in accounts()
        if row["id"] not in CLOSED | UNREACHABLE
    ]
    ranked = sorted(
        ((i, n, p, r, e, e * 1000 // r if r else 0) for i, n, p, r, e in measured),
        key=lambda item: (-item[5], -item[3], item[0]),
    )
    top = ranked[:10]

    def cell(name: object) -> str:
        text = str(name)
        return '"' + text.replace('"', '""') + '"' if "," in text or '"' in text else text

    lines = ["id,name,plan,requests,errors,rate"]
    lines += [f"{i},{cell(n)},{p},{r},{e},{rate}" for i, n, p, r, e, rate in top]
    result = {
        "listed": TOTAL,
        "closed": sorted(CLOSED),
        "unreachable": sorted(UNREACHABLE),
        "requests": sum(item[3] for item in ranked),
        "top": [{"id": item[0], "rate": item[5]} for item in top],
    }
    return "\n".join(lines) + "\n", result


def setup(sandbox: Sandbox) -> None:
    everyone = accounts()
    lock = threading.Lock()
    attempts: dict[str, int] = defaultdict(int)

    def handle(request: Request) -> Response:
        split = urlsplit(request.path)
        parts = [part for part in split.path.split("/") if part]
        if request.method != "GET":
            return Response.json('{"error": "method"}', 405)
        with lock:
            if parts == ["v2", "accounts"]:
                query = parse_qs(split.query)
                if query.get("limit") != [str(PAGE_SIZE)]:
                    return Response.json('{"error": "limit must be 20"}', 400)
                offset = int(query.get("offset", ["-1"])[0])
                if offset < 0 or offset % PAGE_SIZE:
                    return Response.json('{"error": "bad offset"}', 400)
                scripted = PAGE_FAILURES.get(offset, [])
                key = f"page-{offset}"
                status = scripted[attempts[key]] if attempts[key] < len(scripted) else 200
                attempts[key] += 1
                if status != 200:
                    return Response.json('{"error": "try again"}', status)
                body = {"items": everyone[offset : offset + PAGE_SIZE], "total": TOTAL}
                return Response.json(json.dumps(body))
            if len(parts) == 4 and parts[:2] == ["v2", "accounts"] and parts[3] == "usage":
                account = parts[2]
                known = {row["id"] for row in everyone}
                if account not in known or account in CLOSED:
                    return Response.json('{"error": "no such account"}', 404)
                scripted = USAGE_FAILURES.get(account, [])
                status = scripted[attempts[account]] if attempts[account] < len(scripted) else 200
                attempts[account] += 1
                if status != 200:
                    return Response.json('{"error": "try again"}', status)
                requests, errors = usage_of(account)
                return Response.json(json.dumps({"requests": requests, "errors": errors, "plan_note": "x"}))
        return Response.json('{"error": "not found"}', 404)

    sandbox.serve("billing", handle, latency=0.02)


def check_gaps(label: str, times: list[float]) -> Verdict | None:
    for gap, delay in zip((b - a for a, b in pairwise(times)), DELAYS, strict=False):
        if gap < delay - SLACK:
            return Verdict.wrong(f"{label} was repeated after {gap * 1000:.0f} ms, less than {delay * 1000:.0f} ms")
    return None


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    csv_text, wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    path = sandbox.root / "usage.csv"
    if not path.is_file():
        return Verdict.wrong("usage.csv was not written")
    if path.read_bytes() != csv_text.encode():
        return Verdict.wrong(f"usage.csv holds {path.read_text()!r}, not {csv_text!r}")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "usage.csv")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    service = sandbox.services["billing"]
    if service.peak > LIMIT_IN_FLIGHT:
        return Verdict.wrong(f"{service.peak} requests were in flight at once, more than {LIMIT_IN_FLIGHT}")
    pages: dict[int, list[float]] = defaultdict(list)
    usage: dict[str, list[float]] = defaultdict(list)
    page_order: list[int] = []
    for request in service.requests:
        split = urlsplit(request.path)
        parts = [part for part in split.path.split("/") if part]
        if len(parts) == 2:
            offset = int(parse_qs(split.query)["offset"][0])
            if offset not in pages:
                page_order.append(offset)
            pages[offset].append(request.received)
        else:
            usage[parts[2]].append(request.received)
    if page_order != [0, 20, 40]:
        return Verdict.wrong(f"the pages were requested in the order {page_order}")
    for offset, times in pages.items():
        needed = len(PAGE_FAILURES.get(offset, [])) + 1
        if len(times) != needed:
            return Verdict.wrong(f"the page at offset {offset} was requested {len(times)} times, not {needed}")
        if (problem := check_gaps(f"the page at offset {offset}", times)) is not None:
            return problem
    if sorted(usage) != sorted(str(row["id"]) for row in accounts()):
        return Verdict.wrong("the usage requests did not cover every listed account exactly")
    for account, times in usage.items():
        if account in CLOSED:
            needed = 1
        elif account in UNREACHABLE:
            needed = 3
        else:
            needed = len(USAGE_FAILURES.get(account, [])) + 1
        if len(times) != needed:
            return Verdict.wrong(f"the usage of {account} was requested {len(times)} times, not {needed}")
        if (problem := check_gaps(f"the usage of {account}", times)) is not None:
            return problem
    return Verdict.correct()
