import json
import sqlite3
import threading
from collections import defaultdict
from itertools import pairwise

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

TOTAL = 47
PAGE = 20
GONE = {13, 31}
REFUSALS = {22: 1, 40: 2, 45: 3}
DELAYS = (0.05, 0.15)
SLACK = 0.004
LIMIT = 4
SCHEMA = (
    "CREATE TABLE people (id TEXT PRIMARY KEY, name TEXT NOT NULL, email TEXT NOT NULL, team TEXT);"
    "CREATE TABLE audit (seq INTEGER PRIMARY KEY AUTOINCREMENT, id TEXT NOT NULL, op TEXT NOT NULL);"
    "CREATE TRIGGER people_insert AFTER INSERT ON people "
    "BEGIN INSERT INTO audit(id, op) VALUES (new.id, 'insert'); END;"
    "CREATE TRIGGER people_update AFTER UPDATE ON people "
    "BEGIN INSERT INTO audit(id, op) VALUES (new.id, 'update'); END;"
    "CREATE TRIGGER people_delete AFTER DELETE ON people "
    "BEGIN INSERT INTO audit(id, op) VALUES (old.id, 'delete'); END;"
)
type Person = tuple[str, str, str | None]


def identifier(number: int) -> str:
    return f"p-{number:03d}"


def directory(number: int) -> tuple[str, str, str | None]:
    team = ("core", "infra", "web", None)[number % 4]
    email = f"Person{number}@Example.COM" + (" " if number % 5 == 0 else "")
    return f"Person {number:03d}", email, team


def normalized(number: int) -> Person:
    name, email, team = directory(number)
    return name, email.strip().lower(), team


def stored() -> dict[str, Person]:
    rows: dict[str, Person] = {}
    for number in range(3, TOTAL + 1, 3):
        name, email, team = normalized(number)
        if number % 9 == 0:
            name = "Old " + name
        if number % 15 == 0:
            team = "legacy"
        rows[identifier(number)] = (name, email, team)
    for number in sorted(GONE):
        rows[identifier(number)] = ("Stale " + str(number), f"stale{number}@example.com", None)
    rows["p-901"] = ("Departed One", "one@example.com", "web")
    rows["p-902"] = ("Departed Two", "two@example.com", None)
    return rows


def expected() -> tuple[dict[str, object], list[tuple[str, str]], dict[str, Person]]:
    rows = stored()
    final = dict(rows)
    inserted: list[str] = []
    updated: list[str] = []
    unchanged = 0
    audit: list[tuple[str, str]] = []
    for number in range(1, TOTAL + 1):
        name = identifier(number)
        if number in GONE or (number in REFUSALS and REFUSALS[number] >= 3):
            continue
        fresh = normalized(number)
        if name not in rows:
            inserted.append(name)
            audit.append((name, "insert"))
        elif rows[name] != fresh:
            updated.append(name)
            audit.append((name, "update"))
        else:
            unchanged += 1
        final[name] = fresh
    deleted = sorted(name for name in rows if name not in {identifier(n) for n in range(1, TOTAL + 1)})
    for name in deleted:
        del final[name]
        audit.append((name, "delete"))
    result: dict[str, object] = {
        "listed": TOTAL,
        "inserted": inserted,
        "updated": updated,
        "deleted": deleted,
        "unchanged": unchanged,
        "gone": [identifier(n) for n in sorted(GONE)],
        "failed": [identifier(n) for n, count in sorted(REFUSALS.items()) if count >= 3],
    }
    return result, sorted(audit), final


def setup(sandbox: Sandbox) -> None:
    database = sandbox.root / "people.db"
    connection = sqlite3.connect(database)
    try:
        connection.executescript(SCHEMA.split("CREATE TABLE audit")[0])
        connection.executemany("INSERT INTO people(id, name, email, team) VALUES (?, ?, ?, ?)",
                               [(key, *value) for key, value in stored().items()])
        connection.executescript("CREATE TABLE audit" + SCHEMA.split("CREATE TABLE audit")[1])
        connection.commit()
    finally:
        connection.close()
    refused: dict[int, int] = defaultdict(int)
    guard = threading.Lock()

    def handle(request: Request) -> Response:
        path, _, query = request.path.partition("?")
        parts = [part for part in path.split("/") if part]
        if request.method != "GET" or parts[:2] != ["v2", "people"]:
            return Response.json('{"error": "not found"}', 404)
        if len(parts) == 2:
            fields = dict(pair.split("=") for pair in query.split("&"))
            offset, limit = int(fields["offset"]), int(fields["limit"])
            if limit != PAGE or offset % PAGE or offset >= TOTAL:
                return Response.json('{"error": "bad page"}', 400)
            numbers = range(offset + 1, min(offset + PAGE, TOTAL) + 1)
            page = [{"id": identifier(n), "updated": "2026-02-01"} for n in numbers]
            return Response.json(json.dumps({"total": TOTAL, "people": page}))
        number = int(parts[2].removeprefix("p-"))
        if number in GONE or not 1 <= number <= TOTAL:
            return Response.json('{"error": "no such employee"}', 404)
        with guard:
            if refused[number] < REFUSALS.get(number, 0):
                refused[number] += 1
                return Response.json('{"error": "slow down"}', 429)
        name, email, team = directory(number)
        return Response.json(json.dumps({"id": parts[2], "name": name, "email": email, "team": team, "office": "HQ"}))

    sandbox.serve("dir", handle, latency=0.03)


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, audit, final = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    connection = sqlite3.connect(sandbox.root / "people.db")
    try:
        rows = {row[0]: tuple(row[1:]) for row in connection.execute("SELECT id, name, email, team FROM people")}
        log = sorted(connection.execute("SELECT id, op FROM audit").fetchall())
    finally:
        connection.close()
    if rows != final:
        return Verdict.wrong("the people table does not hold the expected rows")
    if log != audit:
        return Verdict.wrong(f"the writes were {log}, not {audit}")
    service = sandbox.services["dir"]
    listing = [request for request in service.requests if request.path.split("?")[0] == "/v2/people"]
    offsets = [int(request.path.split("offset=")[1].split("&")[0]) for request in listing]
    if offsets != list(range(0, TOTAL, PAGE)):
        return Verdict.wrong(f"the pages were requested at offsets {offsets}")
    times: dict[str, list[float]] = defaultdict(list)
    for request in service.requests:
        parts = request.path.split("?")[0].split("/")
        if len(parts) == 4:
            times[parts[3]].append(request.received)
    if sorted(times) != [identifier(n) for n in range(1, TOTAL + 1)]:
        return Verdict.wrong("the details were not fetched for exactly the listed employees")
    for number in range(1, TOTAL + 1):
        count = len(times[identifier(number)])
        needed = 1 if number in GONE else min(REFUSALS.get(number, 0) + 1, 3)
        if count != needed:
            return Verdict.wrong(f"{identifier(number)} was fetched {count} times, not {needed}")
        for gap, delay in zip((b - a for a, b in pairwise(times[identifier(number)])), DELAYS, strict=False):
            if gap < delay - SLACK:
                return Verdict.wrong(f"{identifier(number)} was fetched again after {gap * 1000:.0f} ms")
    if service.peak > LIMIT:
        return Verdict.wrong(f"{service.peak} requests were in flight at once, more than {LIMIT}")
    return Verdict.correct()
