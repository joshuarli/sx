import json
import sys
from pathlib import Path

from definition import Evidence, Verdict
from sandbox import Sandbox

SERVER = Path(__file__).parent / "server.py"
PEOPLE = ["ana", "ben", "chen", "dara"]
DAY = "2026-03-02"
BOOKINGS = [
    {"title": "Planning", "day": DAY, "start": 780, "end": 840, "attendees": ["ana", "ben", "chen"]},
    {"title": "Design review", "day": DAY, "start": 720, "end": 765, "attendees": ["ben", "chen", "dara"]},
    {"title": "Design review", "day": DAY, "start": 735, "end": 780, "attendees": ["ben", "chen", "dara"]},
    {"title": "Design review", "day": DAY, "start": 900, "end": 945, "attendees": ["ben", "chen", "dara"]},
    {"title": "Retro", "day": DAY, "start": 690, "end": 720, "attendees": ["ana", "ben"]},
]
REFUSED = [False, True, True, False, False]
EXPECTED = {
    "meetings": [
        {"title": "Planning", "id": "M-1", "start": 780, "end": 840, "room": "Room A", "refused": 0},
        {"title": "Design review", "id": "M-2", "start": 900, "end": 945, "room": "Room B", "refused": 2},
        {"title": "Retro", "id": "M-3", "start": 690, "end": 720, "room": None, "refused": 0},
    ]
}


def setup(sandbox: Sandbox) -> None:
    sandbox.define("python", sys.executable)
    sandbox.define("server", str(SERVER))
    sandbox.write("ledger.jsonl", "")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    if result != EXPECTED:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(EXPECTED)}")
    entries = [json.loads(line) for line in (sandbox.root / "ledger.jsonl").read_text().splitlines()]
    reads = sorted(str(entry["arguments"]["person"]) for entry in entries if entry["tool"] == "get_availability")
    if reads != PEOPLE:
        return Verdict.wrong(f"each person's availability should be read exactly once; the reads were {reads}")
    if any(entry["arguments"]["day"] != DAY for entry in entries if entry["tool"] == "get_availability"):
        return Verdict.wrong("an availability read named another day")
    bookings = [entry for entry in entries if entry["tool"] == "book_meeting"]
    asked = [{**entry["arguments"], "attendees": sorted(entry["arguments"]["attendees"])} for entry in bookings]
    wanted = [{**booking, "attendees": sorted(booking["attendees"])} for booking in BOOKINGS]
    if asked != wanted:
        return Verdict.wrong(f"the book-meeting calls were {json.dumps(asked)}, not {json.dumps(wanted)}")
    if [bool(entry["error"]) for entry in bookings] != REFUSED:
        return Verdict.wrong("the service refused different bookings than expected")
    return Verdict.correct()
