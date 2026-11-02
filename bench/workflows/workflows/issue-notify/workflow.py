import json
import sqlite3
from urllib.parse import parse_qs, urlsplit

from definition import Evidence, Verdict
from sandbox import Request, Response, Sandbox

PER_PAGE = 20
TOTAL = 87
SEVERITIES = ("low", "high", "low", "high", "high", "low", "low")
TITLES = ("Payment gateway timeout", "Login loop on mobile", "Search index stale", "Export job stuck", "Webhook retries")
SEEDED = (1007, 1013, 1031, 1067, 1500)
FAILING_POST = 1049
SCHEMA = "CREATE TABLE notified (issue_id INTEGER PRIMARY KEY, title TEXT NOT NULL)"
type Issue = dict[str, object]


def issue(number: int) -> Issue:
    return {
        "id": 1000 + number,
        "title": f"{TITLES[number % len(TITLES)]} {number}",
        "severity": "critical" if number % 9 == 4 or number in (40, 60) else SEVERITIES[number % len(SEVERITIES)],
        "state": "closed" if number % 5 == 0 and number != 60 else "open",
        "assignee": f"user-{number % 7}" if number % 3 else None,
    }


def pages() -> list[list[Issue]]:
    issues = [issue(number) for number in range(1, TOTAL + 1)]
    result = [issues[at : at + PER_PAGE] for at in range(0, len(issues), PER_PAGE)]
    stale = result[1][-1]
    result[1][-1] = {**stale, "state": "open"}
    result[2].insert(0, {**stale, "state": "closed"})
    result[3].insert(0, dict(result[2][-1]))
    return result


def current_issues() -> list[Issue]:
    latest: dict[int, Issue] = {}
    for page in pages():
        for item in page:
            latest[int(str(item["id"]))] = item
    return [latest[number] for number in sorted(latest)]


def candidates() -> list[Issue]:
    return [item for item in current_issues() if item["severity"] == "critical" and item["state"] == "open"]


def message(item: Issue) -> str:
    return f"Critical issue #{item['id']}: {item['title']}"


def expected_result() -> object:
    found = candidates()
    already = [int(str(item["id"])) for item in found if item["id"] in SEEDED]
    fresh = [int(str(item["id"])) for item in found if item["id"] not in SEEDED]
    return {
        "pages": len(pages()) + 1,
        "candidates": len(found),
        "already": already,
        "notified": [number for number in fresh if number != FAILING_POST],
        "failed": [FAILING_POST],
    }


def setup(sandbox: Sandbox) -> None:
    data = pages()

    def tracker(request: Request) -> Response:
        split = urlsplit(request.path)
        query = parse_qs(split.query)
        if request.method != "GET" or split.path != "/issues":
            return Response.json('{"error": "not found"}', 404)
        if query.get("per_page") != [str(PER_PAGE)] or not query.get("page", [""])[0].isdigit():
            return Response.json('{"error": "per_page must be 20 and page a number"}', 400)
        number = int(query["page"][0])
        items = data[number - 1] if 1 <= number <= len(data) else []
        return Response.json(json.dumps({"items": items, "page": number}))

    def chat(request: Request) -> Response:
        if request.method != "POST" or request.path != "/messages":
            return Response.json('{"error": "not found"}', 404)
        text = str(json.loads(request.body).get("text"))
        if text.startswith(f"Critical issue #{FAILING_POST}:"):
            return Response.json('{"error": "unavailable"}', 503)
        return Response.json(json.dumps({"id": len(posted(sandbox))}), 201)

    sandbox.serve("tracker", tracker)
    sandbox.serve("chat", chat)
    connection = sqlite3.connect(sandbox.root / "notified.db")
    try:
        connection.execute(SCHEMA)
        connection.executemany(
            "INSERT INTO notified(issue_id, title) VALUES (?, ?)", [(number, "announced earlier") for number in SEEDED]
        )
        connection.commit()
    finally:
        connection.close()


def posted(sandbox: Sandbox) -> list[dict[str, str]]:
    return [json.loads(request.body) for request in sandbox.services["chat"].requests]


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted = expected_result()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    requested = [
        parse_qs(urlsplit(request.path).query).get("page") for request in sandbox.services["tracker"].requests
    ]
    if requested != [[str(number)] for number in range(1, len(pages()) + 2)]:
        return Verdict.wrong(f"the pages were requested as {requested}")
    fresh = [item for item in candidates() if item["id"] not in SEEDED]
    attempts = posted(sandbox)
    wanted_posts = [{"channel": "#oncall", "text": message(item)} for item in fresh]
    if attempts != wanted_posts:
        return Verdict.wrong(f"the chat requests were {json.dumps(attempts)}, not {json.dumps(wanted_posts)}")
    connection = sqlite3.connect(sandbox.root / "notified.db")
    try:
        tables = connection.execute("SELECT name, sql FROM sqlite_master").fetchall()
        rows = connection.execute("SELECT issue_id, title FROM notified ORDER BY issue_id").fetchall()
    finally:
        connection.close()
    if tables != [("notified", SCHEMA)]:
        return Verdict.wrong(f"the database schema changed: {tables}")
    wanted_rows = [(number, "announced earlier") for number in SEEDED]
    wanted_rows += [(int(str(item["id"])), str(item["title"])) for item in fresh if item["id"] != FAILING_POST]
    if rows != sorted(wanted_rows):
        return Verdict.wrong(f"the notified table holds {rows}, not {sorted(wanted_rows)}")
    extra = sorted(path.name for path in sandbox.root.iterdir() if path.name != "notified.db")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
