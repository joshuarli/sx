import json
import sys

PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")
PAGE_SIZE = 4
PEOPLE = [
    {"name": "ana", "skills": ["backend", "infra"], "capacity": 10, "load": 2, "reserved": 0},
    {"name": "bo", "skills": ["backend"], "capacity": 8, "load": 3, "reserved": 0},
    {"name": "cy", "skills": ["frontend", "backend"], "capacity": 6, "load": 1, "reserved": 0},
    {"name": "dee", "skills": ["infra"], "capacity": 5, "load": 0, "reserved": 4},
    {"name": "eli", "skills": ["design"], "capacity": 4, "load": 0, "reserved": 0},
]
TASKS = [
    ("T-101", "Fix login redirect", "urgent", "backend", 3, "open", None),
    ("T-102", "Update the pricing page", "normal", "frontend", 2, "open", None),
    ("T-103", "Rotate database credentials", "high", "infra", 3, "open", None),
    ("T-104", "Tidy the settings module", "low", "backend", 1, "open", None),
    ("T-105", "Add rate limiting", "high", "backend", 3, "open", None),
    ("T-106", "Resize the build cache", "urgent", "infra", 2, "open", None),
    ("T-107", "Redesign the onboarding flow", "normal", "design", 5, "open", None),
    ("T-108", "Review the audit findings", "high", "security", 2, "open", None),
    ("T-109", "Remove the old export", "normal", "backend", 2, "closed", None),
    ("T-110", "Fix the footer links", "urgent", "frontend", 2, "open", "cy"),
    ("T-111", "Migrate the log pipeline", "low", "infra", 4, "open", None),
    ("T-112", "Cache the search results", "normal", "backend", 2, "open", None),
]
PRIORITIES = ["low", "normal", "high", "urgent"]
NULLABLE_STRING = {"anyOf": [{"type": "string"}, {"type": "null"}]}
TOOLS = [
    {
        "name": "list_tasks",
        "description": "Returns one page of tasks and the cursor of the next page, if any.",
        "inputSchema": {"type": "object", "properties": {"cursor": {"type": "string"}}},
        "outputSchema": {
            "type": "object",
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "id": {"type": "string"},
                            "title": {"type": "string"},
                            "priority": {"type": "string", "enum": PRIORITIES},
                            "area": {"type": "string"},
                            "estimate": {"type": "integer"},
                            "state": {"type": "string", "enum": ["open", "closed"]},
                            "assignee": NULLABLE_STRING,
                        },
                        "required": ["id", "title", "priority", "area", "estimate", "state", "assignee"],
                    },
                },
                "next_cursor": NULLABLE_STRING,
            },
            "required": ["items", "next_cursor"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "list_people",
        "description": "Returns everyone who can take tasks, with skills, capacity, and current load.",
        "inputSchema": {"type": "object", "properties": {}},
        "outputSchema": {
            "type": "object",
            "properties": {
                "people": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "skills": {"type": "array", "items": {"type": "string"}},
                            "capacity": {"type": "integer"},
                            "load": {"type": "integer"},
                        },
                        "required": ["name", "skills", "capacity", "load"],
                    },
                }
            },
            "required": ["people"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "assign_task",
        "description": "Assigns a task to a person and returns the person's new load.",
        "inputSchema": {
            "type": "object",
            "properties": {"task_id": {"type": "string"}, "assignee": {"type": "string"}},
            "required": ["task_id", "assignee"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {"task_id": {"type": "string"}, "assignee": {"type": "string"}, "load": {"type": "integer"}},
            "required": ["task_id", "assignee", "load"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": True, "idempotentHint": False},
    },
]


class Desk:
    def __init__(self, ledger: str | None) -> None:
        self.ledger = ledger
        self.people = [dict(person) for person in PEOPLE]
        self.tasks = [
            {"id": i, "title": t, "priority": p, "area": a, "estimate": e, "state": s, "assignee": w}
            for i, t, p, a, e, s, w in TASKS
        ]
        self.calls: list[tuple[str, dict[str, object]]] = []

    def call(self, name: str, arguments: dict[str, object]) -> dict[str, object]:
        self.calls.append((name, arguments))
        if self.ledger is not None:
            with open(self.ledger, "a") as handle:
                handle.write(json.dumps({"tool": name, "arguments": arguments}) + "\n")
        if name == "list_tasks":
            return self.list_tasks(arguments.get("cursor"))
        if name == "list_people":
            people = [{k: v for k, v in p.items() if k != "reserved"} for p in self.people]
            for person, shown in zip(self.people, people, strict=True):
                shown["load"] = person["load"]
            return success({"people": people})
        return self.assign(str(arguments.get("task_id")), str(arguments.get("assignee")))

    def list_tasks(self, cursor: object) -> dict[str, object]:
        start = 0 if cursor is None else int(str(cursor).removeprefix("c"))
        stop = start + PAGE_SIZE
        following = f"c{stop}" if stop < len(self.tasks) else None
        return success({"items": [dict(task) for task in self.tasks[start:stop]], "next_cursor": following})

    def assign(self, task_id: str, assignee: str) -> dict[str, object]:
        task = next((t for t in self.tasks if t["id"] == task_id), None)
        person = next((p for p in self.people if p["name"] == assignee), None)
        if task is None or person is None:
            return failure(f"unknown task or person: {task_id}, {assignee}")
        if task["state"] != "open" or task["assignee"] is not None:
            return failure(f"{task_id} cannot be assigned")
        if int(str(person["load"])) + int(str(person["reserved"])) + int(str(task["estimate"])) > int(
            str(person["capacity"])
        ):
            return failure(f"{assignee} is at capacity")
        person["load"] = int(str(person["load"])) + int(str(task["estimate"]))
        task["assignee"] = assignee
        return success({"task_id": task_id, "assignee": assignee, "load": person["load"]})


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


def serve(desk: Desk) -> None:
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
                "serverInfo": {"name": "taskdesk", "version": "1"},
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
                respond(identifier, desk.call(name, params.get("arguments", {})))
        else:
            respond(identifier, error=(-32601, f"method not found: {method}"))


if __name__ == "__main__":
    serve(Desk(sys.argv[1]))
