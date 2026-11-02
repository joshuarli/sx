import json
import os
import shutil
import subprocess
import sys
from pathlib import Path
from typing import Any

from sx.checker import Budgets
from sx.export import export_program
from sx.program import compile_source
from sx.registry import Registry, load_declarations

ROOT = Path(__file__).resolve().parent.parent
LEAN = ROOT / "lean"
FIXTURES = ROOT / "fixtures" / "run"
SCRATCH = ROOT / ".work" / "tmp" / "agree"
KERNEL_ENVIRONMENT = "SX_KERNEL"
KERNEL_BUILD = "cargo build --release --manifest-path rust/Cargo.toml -p sx-cli"


def lake() -> str:
    found = shutil.which("lake")
    if found is None:
        raise SystemExit("agree.py: lake is not on PATH. Install elan and add ~/.elan/bin to PATH.")
    return found


def rust_kernel() -> Path:
    configured = os.environ.get(KERNEL_ENVIRONMENT)
    if configured is not None:
        return Path(configured)
    for profile in ("release", "debug"):
        candidate = ROOT / "rust" / "target" / profile / "sx"
        if candidate.is_file():
            return candidate
    raise SystemExit(
        f"agree.py: a fixture marked python: false needs the Rust sx. Run `{KERNEL_BUILD}` or set {KERNEL_ENVIRONMENT}."
    )


def rust_ir(case: dict[str, Any], stem: str) -> Path | None:
    program = SCRATCH / f"{stem}.sx"
    program.write_text(case["program"])
    command = [rust_kernel(), "check", program, "--declarations", FIXTURES / case["declarations"], "--emit", "ir"]
    for capability in case["allow"]:
        command += ["--allow", capability]
    emitted = subprocess.run(command, capture_output=True, text=True, check=False)
    if emitted.returncode != 0:
        return None
    ir = SCRATCH / f"{stem}.ir.json"
    ir.write_text(emitted.stdout)
    return ir


def retry_write_twins(binary: Path) -> tuple[int, list[str]]:
    declarations = json.loads((FIXTURES / "declarations.json").read_text())
    unsafe = {c["id"] for c in declarations["capabilities"] if c["effect"] == "write" and not c["idempotent"]}
    for capability in declarations["capabilities"]:
        if capability["id"] in unsafe:
            capability["idempotent"] = True
    registry = Registry()
    load_declarations(registry, declarations)
    accepted_by_lean: list[str] = []
    twins = 0
    for path in sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json"):
        case = json.loads(path.read_text())
        if case.get("python") is False or "retry-write" not in case["expect"].get("rejected", []):
            continue
        checked = compile_source(case["program"], registry, allowed=frozenset(case["allow"]))
        assert checked.program is not None, f"{path.stem}: the twin must check once its writes are idempotent"
        exported = export_program(checked.program, {})
        for signature_id, signature in exported["signatures"]["capabilities"].items():
            if signature_id in unsafe:
                signature["idempotent"] = False
        ir = SCRATCH / f"{path.stem}.twin.ir.json"
        ir.write_text(json.dumps(exported))
        twins += 1
        if subprocess.run([binary, "check", ir], capture_output=True, text=True, check=False).returncode == 0:
            accepted_by_lean.append(path.stem)
    return twins, accepted_by_lean


def lone_surrogate_refused(binary: Path) -> bool:
    registry = Registry()
    load_declarations(registry, json.loads((FIXTURES / "declarations.json").read_text()))
    checked = compile_source("(call store.list {})", registry, allowed=frozenset({"store.list"}))
    assert checked.program is not None
    ir = SCRATCH / "lone-surrogate.ir.json"
    ir.write_text(json.dumps(export_program(checked.program, {})))
    fixture = SCRATCH / "lone-surrogate.json"
    entry = '{"capability": "store.list", "input": {}, "results": [{"output": ["\\ud800"]}]}'
    fixture.write_text('{"responses": [' + entry + "]}")
    ran = subprocess.run([binary, ir, fixture], capture_output=True, text=True, check=False)
    return ran.returncode != 0 and "lone surrogate" in ran.stderr


def outcome_matches(outcome: dict[str, Any], expected: dict[str, Any]) -> bool:
    if "invalid-input" in expected:
        message = outcome.get("invalid-input")
        return isinstance(message, str) and expected["invalid-input"] in message
    return outcome == expected


def decimal_vectors(binary: Path) -> tuple[int, bool]:
    ran = subprocess.run(
        [binary, "decimal-vectors", ROOT / "fixtures" / "cores" / "decimal.json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if ran.stderr:
        print(ran.stderr, end="")
    summary = json.loads(ran.stdout)
    return summary["vectors"], ran.returncode == 0


def json_golden(binary: Path) -> tuple[int, int, bool]:
    golden = ROOT / "rust" / "sx-cores" / "tests" / "json_golden"
    ran = subprocess.run(
        [binary, "json-golden", golden / "supported.json", golden / "refused.json"],
        capture_output=True,
        text=True,
        check=False,
    )
    if ran.stderr:
        print(ran.stderr, end="")
    summary = json.loads(ran.stdout)
    return summary["reads"], summary["refusals"], ran.returncode == 0


def main() -> int:
    subprocess.run([lake(), "build", "sx-ref"], cwd=LEAN, check=True, capture_output=True)
    binary = LEAN / ".lake" / "build" / "bin" / "sx-ref"
    SCRATCH.mkdir(parents=True, exist_ok=True)
    compared, pure_compared, rejected, disagreements, unchecked = 0, 0, [], [], []
    for path in sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json"):
        case = json.loads(path.read_text())
        if case.get("python") is False:
            ir = rust_ir(case, path.stem)
        else:
            registry = Registry()
            load_declarations(registry, json.loads((FIXTURES / case["declarations"]).read_text()))
            budgets = Budgets(**case.get("budgets", {}))
            checked = compile_source(case["program"], registry, allowed=frozenset(case["allow"]), budgets=budgets)
            ir = None
            if checked.program is not None:
                ir = SCRATCH / f"{path.stem}.ir.json"
                ir.write_text(json.dumps(export_program(checked.program, {})))
        if ir is None:
            rejected.append(path.stem)
            continue
        checked_by_lean = subprocess.run([binary, "check", ir], capture_output=True, text=True, check=False)
        if checked_by_lean.returncode != 0:
            unchecked.append((path.stem, checked_by_lean.stderr.strip()))
        if "budgets" not in case and "inputs" not in case:
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
        if not outcome_matches(result["outcome"], case["expect"]):
            disagreements.append((path.stem, f"outcome {result['outcome']} != {case['expect']}"))
        elif result["trace"] != case.get("trace", []):
            disagreements.append((path.stem, f"trace {result['trace']} != {case.get('trace', [])}"))
        elif result["logs"] != case.get("logs", []):
            disagreements.append((path.stem, f"logs {result['logs']} != {case.get('logs', [])}"))
    for name, reason in disagreements:
        print(f"disagree {name}: {reason}")
    for name, reason in unchecked:
        print(f"lean checker rejects {name}: {reason}")
    print(f"compared {compared} accepted fixtures, {len(disagreements)} disagree; "
          f"{len(rejected)} rejected fixtures are elaborator behavior and are not compared: {', '.join(rejected)}")
    print(f"the pure evaluator agrees on {pure_compared} call-free fixtures")
    print(f"the Lean checker accepts {compared - len(unchecked)} of {compared} exported programs")
    twins, wrongly_accepted = retry_write_twins(binary)
    for name in wrongly_accepted:
        print(f"lean checker accepts the retry-write twin of {name}")
    print(f"the Lean checker rejects {twins - len(wrongly_accepted)} of {twins} retry-write twins")
    vectors, vectors_agree = decimal_vectors(binary)
    print(f"the Lean decimal core {'reproduces' if vectors_agree else 'DISAGREES with'} {vectors} decimal vectors")
    reads, refusals, json_agrees = json_golden(binary)
    print(f"the Lean json core {'reproduces' if json_agrees else 'DISAGREES with'} {reads} golden reads and {refusals} golden refusals")
    surrogate_refused = lone_surrogate_refused(binary)
    print(f"the Lean reader {'refuses' if surrogate_refused else 'accepts'} a lone surrogate escape in a string")
    return 1 if disagreements or unchecked or wrongly_accepted or not surrogate_refused or not vectors_agree or not json_agrees else 0


if __name__ == "__main__":
    sys.exit(main())
