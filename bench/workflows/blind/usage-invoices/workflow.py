import json
import sqlite3
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

PER_PAGE = 5
ACCOUNTS: list[tuple[str, str]] = [
    ("acct-001", "free"), ("acct-002", "standard"), ("acct-003", "enterprise"), ("acct-004", "standard"),
    ("acct-005", "enterprise"), ("acct-006", "standard"), ("acct-007", "standard"), ("acct-008", "enterprise"),
    ("acct-009", "standard"), ("acct-010", "standard"), ("acct-011", "enterprise"), ("acct-012", "enterprise"),
]
USAGE: dict[str, list[tuple[str, int]]] = {
    "acct-002": [("api", 1700), ("storage", 3), ("api", 2500)],
    "acct-003": [("api", 52000), ("storage", 20), ("egress", 99999)],
    "acct-004": [("api", 800), ("egress", 5)],
    "acct-005": [("api", 9000), ("storage", 2)],
    "acct-008": [("api", 20000)],
    "acct-009": [("api", 15000), ("storage", 10)],
    "acct-010": [("api", 3000), ("storage", 1)],
    "acct-011": [("api", 20000), ("storage", 5)],
    "acct-012": [("api", 12000), ("storage", 1)],
}
LEDGERED = {"acct-007": ("inv-old-7", 1234)}
UNAVAILABLE_USAGE = {"acct-008": 3, "acct-009": 1}
REFUSED_INVOICES = {"acct-011": 1}
CONFLICT = {"acct-010"}
BROKEN_INVOICE = {"acct-012"}
USAGE_DELAY = 0.05
INVOICE_DELAY = 0.1
SLACK = 0.004
LIMIT = 5


def subtotal(plan: str, events: list[tuple[str, int]]) -> int:
    api = sum(quantity for metric, quantity in events if metric == "api")
    storage = sum(quantity for metric, quantity in events if metric == "storage")
    if plan == "standard":
        return max(min(api, 10000) - 1000, 0) * 3 + max(api - 10000, 0) * 2 + storage * 50
    raw = max(api - 10000, 0) + storage * 30
    return 5000 if 0 < raw < 5000 else raw


def amount(base: int) -> int:
    return base + (base * 7 + 50) // 100


def expected() -> tuple[dict[str, object], list[tuple[str, int]], list[tuple[str, str, int]]]:
    plans = dict(ACCOUNTS)
    unknown = ["acct-006"]
    failed = ["acct-008"]
    no_charge: list[str] = []
    invoiced: list[dict[str, object]] = []
    recorded: list[tuple[str, str, int]] = [(a, i, c) for a, (i, c) in LEDGERED.items()]
    already: list[str] = []
    posts: list[tuple[str, int]] = []
    for account, plan in ACCOUNTS:
        if plan == "free" or account in LEDGERED or account in unknown or account in failed:
            continue
        base = subtotal(plan, USAGE.get(account, []))
        if base == 0:
            no_charge.append(account)
            continue
        cents = amount(base)
        posts.append((account, cents))
        if account in CONFLICT:
            already.append(account)
        elif account in BROKEN_INVOICE:
            failed.append(account)
        else:
            invoiced.append({"account": account, "cents": cents, "invoice": f"inv-{account[-3:]}"})
            recorded.append((account, f"inv-{account[-3:]}", cents))
    result: dict[str, object] = {
        "accounts": len(ACCOUNTS),
        "free": sum(1 for plan in plans.values() if plan == "free"),
        "ledgered": sorted(LEDGERED),
        "unknown": unknown,
        "no_charge": no_charge,
        "already": already,
        "failed": sorted(failed),
        "invoiced": invoiced,
        "total_cents": sum(int(str(i["cents"])) for i in invoiced),
    }
    return result, posts, sorted(recorded)


def setup(sandbox: Sandbox) -> None:
    connection = sqlite3.connect(sandbox.root / "ledger.db")
    try:
        connection.execute(
            "CREATE TABLE invoices (account TEXT PRIMARY KEY, invoice TEXT NOT NULL, cents INTEGER NOT NULL)"
        )
        connection.executemany(
            "INSERT INTO invoices(account, invoice, cents) VALUES (?, ?, ?)",
            [(account, invoice, cents) for account, (invoice, cents) in LEDGERED.items()],
        )
        connection.commit()
    finally:
        connection.close()
    guard = threading.Lock()
    unavailable: dict[str, int] = defaultdict(int)
    refused: dict[str, int] = defaultdict(int)
    created: dict[str, str] = {}

    def handle(request: Request) -> Response:
        path, _, query = request.path.partition("?")
        parts = [part for part in path.split("/") if part]
        if request.method == "GET" and parts == ["accounts"]:
            page = int(query.split("=")[1])
            pages = -(-len(ACCOUNTS) // PER_PAGE)
            if not 1 <= page <= pages:
                return Response.json('{"error": "no such page"}', 404)
            chunk = ACCOUNTS[(page - 1) * PER_PAGE : page * PER_PAGE]
            listed = [{"id": a, "plan": p, "name": a.upper()} for a, p in chunk]
            body = {"page": page, "pages": pages, "accounts": listed}
            return Response.json(json.dumps(body))
        with guard:
            if request.method == "GET" and len(parts) == 2 and parts[0] == "usage":
                account = parts[1]
                if account not in dict(ACCOUNTS) or account == "acct-006":
                    return Response.json('{"error": "unknown account"}', 404)
                if unavailable[account] < UNAVAILABLE_USAGE.get(account, 0):
                    unavailable[account] += 1
                    return Response.json('{"error": "try later"}', 503)
                events = [{"metric": m, "quantity": q, "day": 1} for m, q in USAGE.get(account, [])]
                return Response.json(json.dumps({"account": account, "events": events}))
            if request.method == "POST" and parts == ["invoices"]:
                body = json.loads(request.body)
                account = body["account"]
                if account in CONFLICT:
                    return Response.json('{"error": "already invoiced"}', 409)
                if refused[account] < REFUSED_INVOICES.get(account, 0):
                    refused[account] += 1
                    return Response.json('{"error": "slow down"}', 429)
                if account in BROKEN_INVOICE:
                    return Response.json('{"error": "storage failure"}', 503)
                if account in created:
                    return Response.json('{"error": "duplicate"}', 409)
                created[account] = f"inv-{account[-3:]}"
                return Response.json(json.dumps({"invoice": created[account], "cents": body["cents"]}), 201)
        return Response.json('{"error": "not found"}', 404)

    sandbox.serve("billing", handle, latency=0.03)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, posts, recorded = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    connection = sqlite3.connect(sandbox.root / "ledger.db")
    try:
        rows = sorted(connection.execute("SELECT account, invoice, cents FROM invoices").fetchall())
    finally:
        connection.close()
    if rows != recorded:
        return Verdict.wrong(f"the ledger holds {rows}, not {recorded}")
    service = sandbox.services["billing"]
    pages: list[int] = []
    usage: dict[str, list[float]] = defaultdict(list)
    sent: list[tuple[str, int]] = []
    sent_times: dict[str, list[float]] = defaultdict(list)
    for request in service.requests:
        path, _, query = request.path.partition("?")
        parts = [part for part in path.split("/") if part]
        if parts == ["accounts"]:
            pages.append(int(query.split("=")[1]))
        elif parts[0] == "usage":
            usage[parts[1]].append(request.received)
        else:
            body = json.loads(request.body)
            sent.append((body["account"], body["cents"]))
            sent_times[body["account"]].append(request.received)
    if pages != [1, 2, 3]:
        return Verdict.wrong(f"the account pages requested were {pages}")
    wanted_usage = sorted(a for a, plan in ACCOUNTS if plan != "free" and a not in LEDGERED)
    if sorted(usage) != wanted_usage:
        return Verdict.wrong(f"usage was requested for {sorted(usage)}, not {wanted_usage}")
    for account, times in usage.items():
        needed = 1 if account == "acct-006" else min(UNAVAILABLE_USAGE.get(account, 0) + 1, 3)
        if len(times) != needed:
            return Verdict.wrong(f"the usage of {account} was requested {len(times)} times, not {needed}")
        if any(b - a < USAGE_DELAY - SLACK for a, b in pairwise(times)):
            return Verdict.wrong(f"the usage of {account} was requested again too soon")
    first_refused = {account: len(sent_times[account]) for account in REFUSED_INVOICES}
    expected_sent = [pair for pair in posts for _ in range(1 + REFUSED_INVOICES.get(pair[0], 0))]
    if sent != expected_sent:
        return Verdict.wrong(f"the invoices were requested as {sent}, not {expected_sent}")
    for account, count in first_refused.items():
        times = sent_times[account]
        if count != 2 or times[1] - times[0] < INVOICE_DELAY - SLACK:
            return Verdict.wrong(f"the refused invoice of {account} was repeated too soon")
    if service.peak > LIMIT:
        return Verdict.wrong(f"{service.peak} requests were in flight at once, more than {LIMIT}")
    return Verdict.correct()
