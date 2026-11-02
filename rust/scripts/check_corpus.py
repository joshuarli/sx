import importlib
import json
import re
import sys
from pathlib import Path
from typing import Any

from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx import help as sx_help
from sx.checker import Budgets
from sx.diagnostics import render
from sx.export import export_program
from sx.program import compile_source
from sx.registry import Core, Registry, load_declarations

ROOT = Path(__file__).resolve().parent.parent.parent
CORES: dict[str, Core] = {core.id: core for core in (TEXT, SEMVER, GIT, GITHUB)}
FUZZ_DECLARATIONS = ("fixtures/run/declarations.json", "tests/fuzz/declarations.json")
GENERATED = 2_000
GUIDE_EXAMPLE = re.compile(r"```sx\n(.*?)```\n\nResult: `(.*?)`\n", re.DOTALL)


def build_registry(spec: dict[str, Any]) -> Registry:
    registry = Registry()
    match spec["kind"]:
        case "task":
            task = json.loads((ROOT / "bench" / "codemode" / "tasks" / spec["id"] / "task.json").read_text())
            load_declarations(registry, task)
            for core_id in task.get("cores", []):
                registry.add_core(CORES[core_id])
        case "declarations":
            load_declarations(registry, json.loads((ROOT / spec["path"]).read_text()))
        case "help":
            load_declarations(registry, sx_help.EXAMPLE_DECLARATIONS)
        case "cores":
            for core in CORES.values():
                registry.add_core(core)
        case "fuzz":
            for path in FUZZ_DECLARATIONS:
                load_declarations(registry, json.loads((ROOT / path).read_text()))
        case other:
            raise ValueError(f"unknown registry kind {other!r}")
    return registry


def check_one(case: dict[str, Any], registries: dict[str, Registry]) -> dict[str, Any]:
    key = json.dumps(case["registry"], sort_keys=True)
    if key not in registries:
        registries[key] = build_registry(case["registry"])
    registry = registries[key]
    allowed = case.get("allowed")
    allowed_ids = frozenset(registry.capabilities) if allowed is None else frozenset(allowed)
    budgets = Budgets(**case.get("budgets") or {})
    source = case["source"]
    try:
        checked = compile_source(source, registry, allowed=allowed_ids, budgets=budgets)
    except RecursionError:
        return {"crash": "RecursionError"}
    except Exception as error:
        return {"crash": f"{type(error).__name__}: {error}"}
    if checked.program is None:
        return {"codes": [d.code for d in checked.diagnostics], "rendered": render(source, checked.diagnostics)}
    return {"ir": json.dumps(export_program(checked.program, {}))}


def examples() -> dict[str, Any]:
    entries = [entry.example for entry in sx_help.ENTRIES.values()]
    guide = [source for source, _ in GUIDE_EXAMPLE.findall(sx_help.guide())]
    functions = [
        function.example.program
        for core in CORES.values()
        for function in core.functions
        if function.example is not None
    ]
    return {"help": entries, "guide": guide, "functions": functions}


def generated() -> list[dict[str, Any]]:
    sys.path.insert(0, str(ROOT))
    differential = importlib.import_module("tests.fuzz.differential")
    known = differential.registry()
    cases = [differential.make_case("rust", index, known, 0.1) for index in range(GENERATED)]
    return [{"source": c.source, "allowed": list(c.allow), "budgets": dict(c.budgets)} for c in cases]


def main(manifest: Path, target: Path) -> int:
    if manifest.name == "examples":
        target.write_text(json.dumps(examples()))
        return 0
    if manifest.name == "generated":
        target.write_text(json.dumps(generated()))
        return 0
    cases = json.loads(manifest.read_text())
    registries: dict[str, Registry] = {}
    target.write_text(json.dumps([check_one(case, registries) for case in cases]))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1]), Path(sys.argv[2])))
