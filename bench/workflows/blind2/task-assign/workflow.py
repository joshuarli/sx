import importlib.util
import json
import sys
from pathlib import Path
from typing import Any

from definition import Evidence, Verdict
from sandbox import Sandbox

SERVER = Path(__file__).parent / "server.py"
specification = importlib.util.spec_from_file_location("taskdesk_server", SERVER)
assert specification is not None
assert specification.loader is not None
server = importlib.util.module_from_spec(specification)
specification.loader.exec_module(server)
PRIORITY_RANK = {name: index for index, name in enumerate(reversed(server.PRIORITIES))}


def simulate() -> tuple[dict[str, Any], list[tuple[str, dict[str, Any]]]]:
    desk = server.Desk(None)
    people = {p["name"]: p for p in desk.call("list_people", {})["structuredContent"]["people"]}
    tasks: list[dict[str, Any]] = []
    cursor: Any = None
    while True:
        page = desk.call("list_tasks", {} if cursor is None else {"cursor": cursor})["structuredContent"]
        tasks += page["items"]
        cursor = page["next_cursor"]
        if cursor is None:
            break
    todo = sorted(
        (t for t in tasks if t["state"] == "open" and t["assignee"] is None),
        key=lambda t: (PRIORITY_RANK[t["priority"]], -t["estimate"], t["id"]),
    )
    assigned: list[dict[str, Any]] = []
    unassigned: list[dict[str, Any]] = []
    for task in todo:
        skilled = [p for p in people.values() if task["area"] in p["skills"]]
        roomy = [p for p in skilled if p["capacity"] - p["load"] >= task["estimate"]]
        for person in sorted(roomy, key=lambda p: (p["load"], p["name"])):
            outcome = desk.call("assign_task", {"task_id": task["id"], "assignee": person["name"]})
            if "isError" not in outcome:
                person["load"] = outcome["structuredContent"]["load"]
                assigned.append({"task": task["id"], "assignee": person["name"]})
                break
        else:
            reason = "no-skill" if not skilled else "no-room" if not roomy else "refused"
            unassigned.append({"task": task["id"], "reason": reason})
    final = desk.call("list_people", {})["structuredContent"]["people"]
    result = {
        "assigned": assigned,
        "unassigned": unassigned,
        "loads": [{"name": p["name"], "load": p["load"]} for p in final],
    }
    return result, desk.calls


def setup(sandbox: Sandbox) -> None:
    sandbox.define("python", sys.executable)
    sandbox.define("server", str(SERVER))
    sandbox.write("ledger.jsonl", "")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, calls = simulate()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    entries = [json.loads(line) for line in (sandbox.root / "ledger.jsonl").read_text().splitlines()]
    seen = [(entry["tool"], entry["arguments"]) for entry in entries]
    if seen != calls:
        return Verdict.wrong(f"the calls were {json.dumps(seen)}, not {json.dumps(calls)}")
    return Verdict.correct()
