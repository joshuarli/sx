import json
import sys
from pathlib import Path

from cores import task_registry

from sx.help import tool_listing

TASKS = Path(__file__).resolve().parent.parent.parent / "bench" / "codemode" / "tasks"


def main(target: Path) -> None:
    listings: dict[str, str] = {}
    for directory in sorted(TASKS.iterdir()):
        if directory.name.startswith("fresh-"):
            continue
        task = json.loads((directory / "task.json").read_text())
        ids = [c["id"] for c in task["capabilities"]]
        listings[directory.name] = tool_listing(task_registry(task), ids, task.get("cores", []))
    target.write_text(json.dumps(listings))


if __name__ == "__main__":
    main(Path(sys.argv[1]))
