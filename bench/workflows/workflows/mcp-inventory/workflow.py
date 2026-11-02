import json
import sys
from pathlib import Path

from definition import Evidence, Verdict
from sandbox import Sandbox

SERVER = Path(__file__).parent / "server.py"
ORDER = {"E-500": 2, "A-100": 3, "D-404": 2, "C-300": 5, "B-200": 1}
EXPECTED = {
    "reserved": [
        {"sku": "A-100", "reservation": "R-1", "cents": 750},
        {"sku": "B-200", "reservation": "R-2", "cents": 4200},
        {"sku": "E-500", "reservation": "R-3", "cents": 398},
    ],
    "rejected": [{"sku": "C-300", "reason": "insufficient"}, {"sku": "D-404", "reason": "unknown"}],
    "total": 5348,
}


def setup(sandbox: Sandbox) -> None:
    sandbox.define("python", sys.executable)
    sandbox.define("server", str(SERVER))
    sandbox.write("ledger.jsonl", "")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    if result != EXPECTED:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(EXPECTED)}")
    entries = [json.loads(line) for line in (sandbox.root / "ledger.jsonl").read_text().splitlines()]
    lookups = sorted(str(entry["arguments"]["sku"]) for entry in entries if entry["tool"] == "lookup_item")
    if lookups != sorted(ORDER):
        return Verdict.wrong(f"each sku should be looked up exactly once; the lookups were {lookups}")
    reserves = [entry["arguments"] for entry in entries if entry["tool"] == "reserve_stock"]
    wanted = [
        {"sku": "A-100", "quantity": 3, "priority": "rush"},
        {"sku": "B-200", "quantity": 1},
        {"sku": "E-500", "quantity": 2, "priority": "rush"},
    ]
    if reserves != wanted:
        return Verdict.wrong(f"the reserve-stock calls were {json.dumps(reserves)}, not {json.dumps(wanted)}")
    return Verdict.correct()
