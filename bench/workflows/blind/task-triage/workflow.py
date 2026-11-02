import json
import sys
from pathlib import Path

from definition import Evidence, Verdict
from sandbox import Sandbox

SERVER = Path(__file__).parent / "server.py"
ORDER = (
    ("T-3", "priya", False),
    ("T-9", "omar", True),
    ("T-12", "priya", False),
    ("T-18", "tariq", False),
    ("T-21", "tariq", False),
    ("T-30", "ines", True),
    ("T-100", "omar", False),
    ("T-102", "priya", False),
)
CURSORS = [None, "page-1", "page-2", "page-3", "page-4"]
EXPECTED = {
    "pages": 5,
    "seen": 18,
    "triaged": [
        {"id": "T-3", "assignee": "priya"},
        {"id": "T-9", "assignee": "omar"},
        {"id": "T-12", "assignee": "priya"},
        {"id": "T-21", "assignee": "tariq"},
        {"id": "T-30", "assignee": "ines"},
        {"id": "T-100", "assignee": "omar"},
        {"id": "T-102", "assignee": "priya"},
    ],
    "failed": ["T-18"],
    "load": [
        {"engineer": "ines", "load": 3},
        {"engineer": "omar", "load": 3},
        {"engineer": "priya", "load": 3},
        {"engineer": "tariq", "load": 2},
    ],
}


def calls() -> list[dict[str, object]]:
    wanted: list[dict[str, object]] = [
        {"tool": "list_tasks", "arguments": {} if cursor is None else {"cursor": cursor}} for cursor in CURSORS
    ]
    for identifier, name, p0 in ORDER:
        arguments: dict[str, object] = {"id": identifier, "assignee": name}
        if p0:
            arguments["status"] = "in-progress"
        wanted.append({"tool": "assign_task", "arguments": arguments})
        if p0:
            wanted.append({"tool": "add_comment", "arguments": {"id": identifier, "text": f"Triaged to {name} (p0)"}})
    return wanted


def setup(sandbox: Sandbox) -> None:
    sandbox.define("python", sys.executable)
    sandbox.define("server", str(SERVER))
    sandbox.write("ledger.jsonl", "")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    if result != EXPECTED:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(EXPECTED)}")
    entries = [json.loads(line) for line in (sandbox.root / "ledger.jsonl").read_text().splitlines()]
    wanted = calls()
    if entries != wanted:
        for number, (got, want) in enumerate(zip(entries, wanted, strict=False)):
            if got != want:
                return Verdict.wrong(f"call {number + 1} was {json.dumps(got)}, not {json.dumps(want)}")
        return Verdict.wrong(f"the tracker saw {len(entries)} calls, not {len(wanted)}")
    return Verdict.correct()
