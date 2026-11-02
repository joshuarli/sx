import argparse
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "bench" / "workflows"))

import effects  # noqa: E402


def phase16(results: Path) -> list[dict[str, str]]:
    found: list[dict[str, str]] = []
    for events in sorted(results.glob("*/*/js-*/events.jsonl")):
        run = events.parents[2].name
        found.extend(
            {
                "phase": "16", "run": run, "task": events.parents[1].name,
                "session": events.parent.name, "code": str(execution.args.get("code", "")),
            }
            for execution in effects.executions(effects.read_events(events))
            if execution.tool == effects.CODEMODE and execution.parent is None
        )
    return found


def phase17(results: Path) -> list[dict[str, str]]:
    found: list[dict[str, str]] = []
    for record in sorted(results.glob("*/*/js-*/record.json")):
        loaded = json.loads(record.read_text())
        found.extend(
            {
                "phase": "17", "run": record.parents[2].name, "task": loaded["request"],
                "session": record.parent.name, "code": program["source"],
            }
            for program in loaded["programs"]
            if program["tool"] == effects.CODEMODE
        )
    return found


def main() -> None:
    parser = argparse.ArgumentParser(description="Collect the codemode scripts the JavaScript arms wrote.")
    parser.add_argument("--effects", type=Path, required=True)
    parser.add_argument("--real", type=Path, required=True)
    parser.add_argument("--out", type=Path, required=True)
    args = parser.parse_args()
    scripts = phase16(args.effects) + phase17(args.real)
    args.out.write_text("".join(json.dumps(script) + "\n" for script in scripts))
    print(f"{len(scripts)} scripts")


if __name__ == "__main__":
    main()
