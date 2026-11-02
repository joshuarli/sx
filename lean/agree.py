import json
import shutil
import subprocess
import sys
from pathlib import Path

from sx.checker import Budgets
from sx.export import export_program
from sx.program import compile_source
from sx.registry import Registry, load_declarations

ROOT = Path(__file__).resolve().parent.parent
LEAN = ROOT / "lean"
FIXTURES = ROOT / "fixtures" / "run"
SCRATCH = ROOT / ".work" / "tmp" / "agree"


def lake() -> str:
    found = shutil.which("lake")
    if found is None:
        raise SystemExit("agree.py: lake is not on PATH. Install elan and add ~/.elan/bin to PATH.")
    return found


def main() -> int:
    subprocess.run([lake(), "build", "sx-ref"], cwd=LEAN, check=True, capture_output=True)
    binary = LEAN / ".lake" / "build" / "bin" / "sx-ref"
    SCRATCH.mkdir(parents=True, exist_ok=True)
    compared, pure_compared, rejected, disagreements, unchecked = 0, 0, [], [], []
    for path in sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json"):
        case = json.loads(path.read_text())
        registry = Registry()
        load_declarations(registry, json.loads((FIXTURES / case["declarations"]).read_text()))
        budgets = Budgets(**case.get("budgets", {}))
        checked = compile_source(case["program"], registry, allowed=frozenset(case["allow"]), budgets=budgets)
        if checked.program is None:
            rejected.append(path.stem)
            continue
        ir = SCRATCH / f"{path.stem}.ir.json"
        ir.write_text(json.dumps(export_program(checked.program, {})))
        checked_by_lean = subprocess.run([binary, "check", ir], capture_output=True, text=True, check=False)
        if checked_by_lean.returncode != 0:
            unchecked.append((path.stem, checked_by_lean.stderr.strip()))
        if "budgets" not in case:
            pure = subprocess.run([binary, "pure", ir], capture_output=True, text=True, check=True)
            evaluated = json.loads(pure.stdout)
            if "outcome" in evaluated:
                pure_compared += 1
                if evaluated["outcome"] != case["expect"]:
                    disagreements.append((path.stem, f"pure evaluator {evaluated['outcome']} != {case['expect']}"))
        ran = subprocess.run([binary, ir, path], capture_output=True, text=True, check=False)
        compared += 1
        if ran.returncode != 0:
            disagreements.append((path.stem, ran.stderr.strip()))
            continue
        result = json.loads(ran.stdout)
        if result["outcome"] != case["expect"]:
            disagreements.append((path.stem, f"outcome {result['outcome']} != {case['expect']}"))
        elif result["trace"] != case["trace"]:
            disagreements.append((path.stem, f"trace {result['trace']} != {case['trace']}"))
    for name, reason in disagreements:
        print(f"disagree {name}: {reason}")
    for name, reason in unchecked:
        print(f"lean checker rejects {name}: {reason}")
    print(f"compared {compared} accepted fixtures, {len(disagreements)} disagree; "
          f"{len(rejected)} rejected fixtures are elaborator behavior and are not compared: {', '.join(rejected)}")
    print(f"the pure evaluator agrees on {pure_compared} call-free fixtures")
    print(f"the Lean checker accepts {compared - len(unchecked)} of {compared} exported programs")
    return 1 if disagreements or unchecked else 0


if __name__ == "__main__":
    sys.exit(main())
