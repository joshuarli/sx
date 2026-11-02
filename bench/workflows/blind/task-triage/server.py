import json
import sys

ROWS: list[tuple[str, str, list[str], str, str | None, str]] = [
    ("T-110", "Search crashes", ["bug"], "p2", "ines", "in-progress"),
    ("T-3", "Typo on login", ["bug", "ui"], "p2", None, "open"),
    ("T-25", "Slow export", ["bug"], "p1", "tariq", "open"),
    ("T-5", "Dark mode", ["feature"], "p1", None, "open"),
    ("T-100", "Date parsing", ["bug"], "p2", None, "open"),
    ("T-9", "Data loss on save", ["bug"], "p0", None, "open"),
    ("T-10", "Flaky test", ["bug", "ci"], "p1", "ines", "open"),
    ("T-101", "Capitalized label", ["Bug"], "p2", None, "open"),
    ("T-11", "Old crash", ["bug"], "p2", None, "done"),
    ("T-12", "Wrong total", ["bug", "billing"], "p1", None, "open"),
    ("T-14", "Outage", ["bug"], "p0", "omar", "in-progress"),
    ("T-17", "Update the docs", ["docs"], "p3", None, "open"),
    ("T-18", "Locked by legal", ["bug"], "p1", None, "open"),
    ("T-102", "Rounding", ["bug"], "p1", None, "open"),
    ("T-21", "Memory leak", ["bug"], "p2", None, "open"),
    ("T-22", "Waiting for vendor", ["bug"], "p0", None, "blocked"),
    ("T-30", "Security hole", ["bug", "security"], "p0", None, "open"),
    ("T-31", "Rotate keys", ["chore"], "p2", "priya", "done"),
]
TASKS: list[dict[str, object]] = [
    {"id": i, "title": t, "labels": labels, "priority": pr, "assignee": a, "status": st}
    for i, t, labels, pr, a, st in ROWS
]
LOCKED = {"T-18"}
PAGE_SIZE = 4
PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")
STRING = {"type": "string"}
NULLABLE_STRING = {"type": ["string", "null"]}
TASK = {
    "type": "object",
    "properties": {
        "id": STRING,
        "title": STRING,
        "labels": {"type": "array", "items": STRING},
        "priority": STRING,
        "assignee": NULLABLE_STRING,
        "status": STRING,
    },
    "required": ["id", "title", "labels", "priority", "assignee", "status"],
}
TOOLS = [
    {
        "name": "list_tasks",
        "description": "Returns one page of tasks.",
        "inputSchema": {"type": "object", "properties": {"cursor": STRING}},
        "outputSchema": {
            "type": "object",
            "properties": {"tasks": {"type": "array", "items": TASK}, "next_cursor": NULLABLE_STRING},
            "required": ["tasks", "next_cursor"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "assign_task",
        "description": "Assigns a task to an engineer, optionally setting its status.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "id": STRING,
                "assignee": STRING,
                "status": {"type": "string", "enum": ["open", "in-progress"]},
            },
            "required": ["id", "assignee"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {"id": STRING, "assignee": STRING, "status": STRING},
            "required": ["id", "assignee", "status"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": True, "idempotentHint": False},
    },
    {
        "name": "add_comment",
        "description": "Adds a comment to a task.",
        "inputSchema": {
            "type": "object",
            "properties": {"id": STRING, "text": STRING},
            "required": ["id", "text"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {"id": STRING, "comments": {"type": "integer"}},
            "required": ["id", "comments"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": True, "idempotentHint": False},
    },
]


class Tracker:
    def __init__(self, ledger: str) -> None:
        self.ledger = ledger
        self.tasks = {str(task["id"]): dict(task) for task in TASKS}
        self.comments: dict[str, int] = {}

    def record(self, name: str, arguments: dict[str, object]) -> None:
        with open(self.ledger, "a") as handle:
            handle.write(json.dumps({"tool": name, "arguments": arguments}) + "\n")

    def call(self, name: str, arguments: dict[str, object]) -> dict[str, object]:
        self.record(name, arguments)
        if name == "list_tasks":
            cursor = arguments.get("cursor")
            start = 0 if cursor is None else int(str(cursor).removeprefix("page-")) * PAGE_SIZE
            if cursor is not None and not str(cursor).startswith("page-"):
                return failure(f"bad cursor: {cursor}")
            page = [self.tasks[str(task["id"])] for task in TASKS[start:start + PAGE_SIZE]]
            following = f"page-{start // PAGE_SIZE + 1}" if start + PAGE_SIZE < len(TASKS) else None
            return success({"tasks": page, "next_cursor": following})
        identifier = str(arguments.get("id"))
        task = self.tasks.get(identifier)
        if task is None:
            return failure(f"unknown task: {identifier}")
        if name == "assign_task":
            if identifier in LOCKED:
                return failure(f"task {identifier} is locked")
            if task["assignee"] is not None:
                return failure(f"task {identifier} is already assigned to {task['assignee']}")
            task["assignee"] = arguments["assignee"]
            if "status" in arguments:
                task["status"] = arguments["status"]
            return success({"id": identifier, "assignee": task["assignee"], "status": task["status"]})
        self.comments[identifier] = self.comments.get(identifier, 0) + 1
        return success({"id": identifier, "comments": self.comments[identifier]})


def success(value: dict[str, object]) -> dict[str, object]:
    return {"content": [{"type": "text", "text": json.dumps(value)}], "structuredContent": value}


def failure(message: str) -> dict[str, object]:
    return {"content": [{"type": "text", "text": message}], "isError": True}


def respond(identifier: object, result: object = None, error: tuple[int, str] | None = None) -> None:
    message: dict[str, object] = {"jsonrpc": "2.0", "id": identifier}
    if error is not None:
        message["error"] = {"code": error[0], "message": error[1]}
    else:
        message["result"] = result
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def serve(tracker: Tracker) -> None:
    for line in sys.stdin:
        request = json.loads(line)
        method, identifier = request.get("method"), request.get("id")
        if identifier is None:
            continue
        if method == "initialize":
            asked = request.get("params", {}).get("protocolVersion")
            version = asked if asked in PROTOCOLS else PROTOCOLS[0]
            respond(identifier, {
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "tracker", "version": "1"},
            })
        elif method == "ping":
            respond(identifier, {})
        elif method == "tools/list":
            respond(identifier, {"tools": TOOLS})
        elif method == "tools/call":
            params = request.get("params", {})
            name = params.get("name")
            if name not in {tool["name"] for tool in TOOLS}:
                respond(identifier, error=(-32602, f"unknown tool: {name}"))
            else:
                respond(identifier, tracker.call(name, params.get("arguments", {})))
        else:
            respond(identifier, error=(-32601, f"method not found: {method}"))


if __name__ == "__main__":
    serve(Tracker(sys.argv[1]))
