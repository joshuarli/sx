import json
import sys

DAY = "2026-03-02"
DEFAULT_HOURS = (540, 1020)
WORKING_HOURS = {"ana": (600, 1020), "dara": (540, 960)}
BUSY = {
    "ana": [(600, 660, "Standup"), (720, 780, "1:1")],
    "ben": [(540, 570, "Inbox"), (630, 690, "Interview")],
    "chen": [(540, 600, "Focus"), (660, 720, "Dentist")],
    "dara": [(540, 630, "Sync"), (840, 900, "Lunch")],
}
HOLDS = {
    "chen": [(720, 735, "tentative hold")],
    "ben": [(750, 780, "tentative hold")],
}
PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")
MINUTE = {"type": "integer"}
TEXT = {"type": "string"}
INTERVAL = {
    "type": "object",
    "properties": {"start": MINUTE, "end": MINUTE, "title": TEXT},
    "required": ["start", "end", "title"],
}
HOURS = {"type": "object", "properties": {"start": MINUTE, "end": MINUTE}, "required": ["start", "end"]}
MEETING = {
    "type": "object",
    "properties": {
        "id": TEXT,
        "title": TEXT,
        "day": TEXT,
        "start": MINUTE,
        "end": MINUTE,
        "attendees": {"type": "array", "items": TEXT},
        "room": TEXT,
    },
    "required": ["id", "title", "day", "start", "end", "attendees"],
}
TOOLS = [
    {
        "name": "get_availability",
        "description": "Returns the busy intervals of one person on one day.",
        "inputSchema": {
            "type": "object",
            "properties": {"person": TEXT, "day": TEXT},
            "required": ["person", "day"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {
                "person": TEXT,
                "day": TEXT,
                "working_hours": HOURS,
                "busy": {"type": "array", "items": INTERVAL},
            },
            "required": ["person", "day", "busy"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "book_meeting",
        "description": "Books a meeting for several people.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "title": TEXT,
                "day": TEXT,
                "start": MINUTE,
                "end": MINUTE,
                "attendees": {"type": "array", "items": TEXT},
            },
            "required": ["title", "day", "start", "end", "attendees"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {"meeting": MEETING},
            "required": ["meeting"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": False, "idempotentHint": False},
    },
]


class Calendar:
    def __init__(self, ledger: str) -> None:
        self.ledger = ledger
        self.booked: dict[str, list[tuple[int, int, str]]] = {name: [] for name in BUSY}
        self.meetings = 0

    def record(self, name: str, arguments: dict[str, object], error: bool) -> None:
        with open(self.ledger, "a") as handle:
            handle.write(json.dumps({"tool": name, "arguments": arguments, "error": error}) + "\n")

    def call(self, name: str, arguments: dict[str, object]) -> dict[str, object]:
        result = self.availability(arguments) if name == "get_availability" else self.book(arguments)
        self.record(name, arguments, bool(result.get("isError")))
        return result

    def availability(self, arguments: dict[str, object]) -> dict[str, object]:
        person, day = str(arguments.get("person")), arguments.get("day")
        if person not in BUSY or day != DAY:
            return failure(f"no calendar for {person} on {day}")
        value: dict[str, object] = {
            "person": person,
            "day": DAY,
            "busy": [
                {"start": start, "end": end, "title": title}
                for start, end, title in sorted(BUSY[person] + self.booked[person])
            ],
        }
        if person in WORKING_HOURS:
            value["working_hours"] = {"start": WORKING_HOURS[person][0], "end": WORKING_HOURS[person][1]}
        return success(value)

    def book(self, arguments: dict[str, object]) -> dict[str, object]:
        title, day = str(arguments.get("title")), arguments.get("day")
        start, end, attendees = arguments.get("start"), arguments.get("end"), arguments.get("attendees")
        if day != DAY or not isinstance(start, int) or not isinstance(end, int) or not isinstance(attendees, list):
            return failure("invalid booking")
        for person in attendees:
            if person not in BUSY:
                return failure(f"unknown person: {person}")
            low, high = WORKING_HOURS.get(person, DEFAULT_HOURS)
            if start < low or end > high or end <= start:
                return failure(f"{person} does not work from {start} to {end}")
            for begin, finish, label in BUSY[person] + HOLDS.get(person, []) + self.booked[person]:
                if start < finish and begin < end:
                    return failure(f"{person} is not free from {begin} to {finish}: {label}")
        self.meetings += 1
        meeting: dict[str, object] = {
            "id": f"M-{self.meetings}",
            "title": title,
            "day": DAY,
            "start": start,
            "end": end,
            "attendees": attendees,
        }
        if len(attendees) >= 3:
            meeting["room"] = f"Room {'AB'[(self.meetings - 1) % 2]}"
        for person in attendees:
            self.booked[person].append((start, end, title))
        return success({"meeting": meeting})


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


def serve(calendar: Calendar) -> None:
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
                "serverInfo": {"name": "calendar", "version": "1"},
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
                respond(identifier, calendar.call(name, params.get("arguments", {})))
        else:
            respond(identifier, error=(-32601, f"method not found: {method}"))


if __name__ == "__main__":
    serve(Calendar(sys.argv[1]))
