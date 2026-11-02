import contextlib
import io
import json
import sys
from pathlib import Path

from sx.cli import main as sx_main

ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES = ROOT / "fixtures" / "run"


def main(target: Path) -> int:
    target.mkdir(parents=True, exist_ok=True)
    for path in sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json"):
        case = json.loads(path.read_text())
        program = target / f"{path.stem}.sx"
        program.write_text(case["program"])
        arguments = ["explain", str(program), "--declarations", str(FIXTURES / case["declarations"]), "--format", "json"]
        for capability in case["allow"]:
            arguments += ["--allow", capability]
        captured = io.StringIO()
        with contextlib.redirect_stdout(captured):
            code = sx_main(arguments)
        (target / f"{path.stem}.explain.json").write_text(json.dumps({"code": code, "output": json.loads(captured.getvalue())}))
    return 0


if __name__ == "__main__":
    sys.exit(main(Path(sys.argv[1])))
