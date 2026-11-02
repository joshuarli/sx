import json
import sqlite3
from collections import defaultdict
from itertools import pairwise
from urllib.parse import parse_qs, urlsplit

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

PAGE_SIZE = 25
TOTAL = 130
OVERLAPS = frozenset({2, 4})
STATUSES = ("pending", "paid", "shipped", "refunded")
FAILURES: dict[str | None, list[int]] = {None: [429], "pg-2": [429, 503], "pg-4": [503]}
DELAYS = (0.05, 0.1, 0.2)
SLACK = 0.004
SCHEMA = (
    "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT NOT NULL, cents INTEGER NOT NULL, "
    "status TEXT NOT NULL, note TEXT)"
)
type Row = dict[str, object]


def order(number: int, revision: int = 0) -> Row:
    return {
        "id": number,
        "customer": f"customer-{number % 17:02d}",
        "cents": 500 + number * 37 % 4000,
        "status": STATUSES[(number * 7 + revision * 2) % len(STATUSES)],
        "note": f"gift {number}" if number % 9 == 0 else None,
    }


def pages() -> list[list[Row]]:
    result: list[list[Row]] = []
    for page in range(-(-TOTAL // PAGE_SIZE)):
        start, stop = page * PAGE_SIZE + 1, min((page + 1) * PAGE_SIZE, TOTAL)
        items = [order(number) for number in range(start, stop + 1)]
        if page in OVERLAPS:
            items.insert(0, order(start - 1, revision=1))
        result.append(items)
    return result


def expected_rows() -> dict[int, Row]:
    return {int(str(item["id"])): item for page in pages() for item in page}


def expected_result() -> Row:
    totals: dict[str, list[int]] = defaultdict(lambda: [0, 0])
    for row in expected_rows().values():
        totals[str(row["status"])][0] += 1
        totals[str(row["status"])][1] += int(str(row["cents"]))
    statuses = [{"status": name, "n": n, "cents": cents} for name, (n, cents) in sorted(totals.items())]
    return {"pages": len(pages()), "fetched": sum(len(page) for page in pages()), "stored": TOTAL, "statuses": statuses}


def setup(sandbox: Sandbox) -> None:
    data = pages()
    attempts: dict[str | None, int] = defaultdict(int)

    def orders(request: Request) -> Response:
        split = urlsplit(request.path)
        query = parse_qs(split.query)
        if request.method != "GET" or split.path != "/v1/orders":
            return Response.json('{"error": "not found"}', 404)
        cursor = cursor_of(request)
        scripted = FAILURES.get(cursor, [])
        status = scripted[attempts[cursor]] if attempts[cursor] < len(scripted) else 200
        attempts[cursor] += 1
        if status != 200:
            return Response.json(json.dumps({"error": "try again"}), status)
        if query.get("limit") != [str(PAGE_SIZE)]:
            return Response.json('{"error": "limit must be 25"}', 400)
        index = 0 if cursor is None else int(cursor.removeprefix("pg-"))
        following = f"pg-{index + 1}" if index + 1 < len(data) else None
        return Response.json(json.dumps({"items": data[index], "next": following}))

    sandbox.serve("api", orders)


def cursor_of(request: Request) -> str | None:
    values = parse_qs(urlsplit(request.path).query).get("cursor")
    return None if values is None else values[0]


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted = expected_result()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    database = sandbox.root / "orders.db"
    if not database.is_file():
        return Verdict.wrong("orders.db was not created")
    connection = sqlite3.connect(database)
    try:
        tables = connection.execute("SELECT name, sql FROM sqlite_master").fetchall()
        rows = connection.execute("SELECT id, customer, cents, status, note FROM orders ORDER BY id").fetchall()
    finally:
        connection.close()
    if tables != [("orders", SCHEMA)]:
        return Verdict.wrong(f"the database holds {tables}, not the one table created by the stated SQL")
    keys = ("id", "customer", "cents", "status", "note")
    stored = [dict(zip(keys, row, strict=True)) for row in rows]
    if stored != [expected_rows()[number] for number in sorted(expected_rows())]:
        return Verdict.wrong("the rows of the orders table are not the latest copy of every fetched order")
    by_cursor: dict[str | None, list[float]] = defaultdict(list)
    sequence: list[str | None] = []
    for request in sandbox.services["api"].requests:
        cursor = cursor_of(request)
        if cursor not in by_cursor:
            sequence.append(cursor)
        by_cursor[cursor].append(request.received)
    if sequence != [None, *(f"pg-{number}" for number in range(1, len(pages())))]:
        return Verdict.wrong(f"the pages were requested in the order {sequence}")
    for cursor, times in by_cursor.items():
        needed = len(FAILURES.get(cursor, [])) + 1
        if len(times) != needed:
            return Verdict.wrong(f"page {cursor} was requested {len(times)} times, not {needed}")
        for gap, delay in zip((b - a for a, b in pairwise(times)), DELAYS, strict=False):
            if gap < delay - SLACK:
                return Verdict.wrong(
                    f"page {cursor} was retried after {gap * 1000:.0f} ms, less than {delay * 1000:.0f} ms"
                )
    extra = sorted(path.name for path in sandbox.root.iterdir() if path.name != "orders.db")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
