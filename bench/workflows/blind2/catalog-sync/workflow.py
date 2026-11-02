import json
import sqlite3
import threading
from collections import defaultdict
from itertools import pairwise
from urllib.parse import parse_qs, urlsplit

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

START = 40
PAGE_SIZE = 10
DELAYS = (0.04, 0.08)
SLACK = 0.005
FAILURES: dict[int, list[int]] = {50: [503], 70: [503, 503, 503, 503]}
INACTIVE = frozenset({"p-012", "p-015"})
type Change = tuple[int, str, str, str | None, int | None, int | None]


def product(number: int) -> tuple[str, str, int, int]:
    return f"p-{number:03d}", f"Item {number:03d}", 1000 + number * 10, number % 7 * 5


def up(seq: int, number: int, name: str | None = None, price: int | None = None, stock: int | None = None) -> Change:
    sku, default_name, default_price, default_stock = product(number)
    return (
        seq,
        "upsert",
        sku,
        name or default_name,
        default_price + 500 if price is None else price,
        default_stock + 1 if stock is None else stock,
    )


def down(seq: int, number: int) -> Change:
    return seq, "delete", f"p-{number:03d}", None, None, None


CHANGES: list[Change] = [
    up(41, 3, "Item 003 v2", 1500, 9),
    up(42, 26),
    down(43, 10),
    up(44, 12),
    up(45, 27),
    down(46, 1),
    up(47, 3, "Item 003 v3", 1550, 8),
    up(48, 28),
    down(49, 999),
    up(50, 26, "Item 026 renamed", 2650, 5),
    down(51, 7),
    up(52, 29),
    down(53, 20),
    up(54, 15),
    down(55, 21),
    up(56, 30),
    up(57, 31),
    up(58, 7, "Item 007 back", 1070, 10),
    down(59, 22),
    up(60, 30, "Item 030 final", 3000, 2),
    up(61, 32),
    up(62, 33),
    down(63, 32),
    up(64, 2),
    down(65, 2),
    up(66, 34),
    down(67, 33),
    up(68, 4),
    down(69, 5),
    up(70, 6),
    up(71, 40),
    up(72, 41),
    down(73, 9),
    up(74, 42),
    up(75, 43),
    down(76, 8),
    up(77, 44),
    up(78, 45),
]
SCHEMA = """
CREATE TABLE products (
  sku TEXT PRIMARY KEY, name TEXT NOT NULL, price_cents INTEGER NOT NULL, stock INTEGER NOT NULL,
  active INTEGER NOT NULL);
CREATE TABLE sync_state (version INTEGER NOT NULL);
CREATE TABLE audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, action TEXT NOT NULL, ref TEXT NOT NULL);
CREATE TRIGGER products_insert AFTER INSERT ON products BEGIN
  INSERT INTO audit(tbl, action, ref) VALUES ('products', 'insert', NEW.sku);
END;
CREATE TRIGGER products_update AFTER UPDATE ON products BEGIN
  INSERT INTO audit(tbl, action, ref) VALUES ('products', 'update', NEW.sku);
END;
CREATE TRIGGER sync_update AFTER UPDATE ON sync_state BEGIN
  INSERT INTO audit(tbl, action, ref) VALUES ('sync', 'update', CAST(NEW.version AS TEXT));
END;
"""


def pages_of(version: int) -> list[Change]:
    return [change for change in CHANGES if change[0] > version][:PAGE_SIZE]


def expected() -> tuple[dict[str, object], list[tuple[str, str, int, int, int]], list[tuple[str, str, str]]]:
    products = {
        sku: [sku, name, price, stock, 0 if sku in INACTIVE else 1]
        for sku, name, price, stock in map(product, range(1, 26))
    }
    audit: list[tuple[str, str, str]] = []
    version = START
    pages = upserted = discontinued = 0
    completed = False
    while True:
        if version in FAILURES and len(FAILURES[version]) > 2:
            break
        page = pages_of(version)
        kept = [c for c in page if not any(d[2] == c[2] and d[0] > c[0] for d in page)]
        for _, op, sku, name, price, stock in kept:
            if op == "upsert":
                audit.append(("products", "update" if sku in products else "insert", sku))
                products[sku] = [sku, str(name), int(str(price)), int(str(stock)), 1]
                upserted += 1
            else:
                if sku in products:
                    products[sku][4] = 0
                    audit.append(("products", "update", sku))
                discontinued += 1
        version = page[-1][0]
        audit.append(("sync", "update", str(version)))
        pages += 1
        if not any(c[0] > version for c in CHANGES):
            completed = True
            break
    rows = sorted((str(v[0]), str(v[1]), int(str(v[2])), int(str(v[3])), int(str(v[4]))) for v in products.values())
    result: dict[str, object] = {
        "since": START,
        "pages": pages,
        "applied": upserted + discontinued,
        "upserted": upserted,
        "discontinued": discontinued,
        "version": version,
        "active": sum(1 for r in rows if r[4] == 1),
        "completed": completed,
    }
    return result, rows, sorted(audit)


def setup(sandbox: Sandbox) -> None:
    connection = sqlite3.connect(sandbox.root / "catalog.db")
    try:
        connection.executescript(SCHEMA)
        for number in range(1, 26):
            sku, name, price, stock = product(number)
            connection.execute(
                "INSERT INTO products VALUES (?, ?, ?, ?, ?)", (sku, name, price, stock, 0 if sku in INACTIVE else 1)
            )
        connection.execute("INSERT INTO sync_state VALUES (?)", (START,))
        connection.execute("DELETE FROM audit")
        connection.commit()
    finally:
        connection.close()
    lock = threading.Lock()
    attempts: dict[int, int] = defaultdict(int)

    def handle(request: Request) -> Response:
        split = urlsplit(request.path)
        query = parse_qs(split.query)
        if request.method != "GET" or split.path != "/changes":
            return Response.json('{"error": "not found"}', 404)
        if query.get("limit") != [str(PAGE_SIZE)]:
            return Response.json('{"error": "limit must be 10"}', 400)
        since = int(query["since"][0])
        with lock:
            scripted = FAILURES.get(since, [])
            status = scripted[attempts[since]] if attempts[since] < len(scripted) else 200
            attempts[since] += 1
        if status != 200:
            return Response.json('{"error": "try again"}', status)
        page = pages_of(since)
        body = {
            "version": page[-1][0] if page else since,
            "more": any(c[0] > page[-1][0] for c in CHANGES) if page else False,
            "changes": [
                {"seq": s, "op": op, "sku": sku, "name": name, "price_cents": price, "stock": stock}
                for s, op, sku, name, price, stock in page
            ],
        }
        return Response.json(json.dumps(body))

    sandbox.serve("supplier", handle)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, rows, audit = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    connection = sqlite3.connect(sandbox.root / "catalog.db")
    try:
        stored = sorted(connection.execute("SELECT sku, name, price_cents, stock, active FROM products").fetchall())
        version = connection.execute("SELECT version FROM sync_state").fetchall()
        trail = sorted(connection.execute("SELECT tbl, action, ref FROM audit").fetchall())
    finally:
        connection.close()
    if stored != rows:
        return Verdict.wrong("the products table does not hold the expected rows")
    if version != [(wanted["version"],)]:
        return Verdict.wrong(f"the stored version is {version}")
    if trail != audit:
        return Verdict.wrong(f"the changes made to the database were {trail}, not {audit}")
    sinces: list[int] = []
    times: dict[int, list[float]] = defaultdict(list)
    for request in sandbox.services["supplier"].requests:
        since = int(parse_qs(urlsplit(request.path).query)["since"][0])
        sinces.append(since)
        times[since].append(request.received)
    wanted_sinces = [40, 50, 50, 60, 70, 70, 70]
    if sinces != wanted_sinces:
        return Verdict.wrong(f"the requests asked for since {sinces}, not {wanted_sinces}")
    for since, stamps in times.items():
        for gap, delay in zip((b - a for a, b in pairwise(stamps)), DELAYS, strict=False):
            if gap < delay - SLACK:
                return Verdict.wrong(f"the page after {since} was repeated after {gap * 1000:.0f} ms")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "catalog.db")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
