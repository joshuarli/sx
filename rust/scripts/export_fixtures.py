import json
import sys
from pathlib import Path
from typing import Any

from sx import ir
from sx.checker import Budgets
from sx.export import _Exporter, export_program
from sx.host import FixtureHost, run
from sx.machine import Completed, Failed, Machine, Outcome, Output, Refusal, Response, Suspended
from sx.program import compile_source
from sx.registry import Registry, load_declarations
from sx.types import Type
from sx.values import DecodeError, decode, encode, show_value

ROOT = Path(__file__).resolve().parent.parent.parent
FIXTURES = ROOT / "fixtures" / "run"


def value_case(t: Type, value: object) -> dict[str, Any]:
    exporter = _Exporter()
    exported = exporter.type(t)
    try:
        decoded = json.dumps(encode(decode(value, t), t))
        error = None
    except DecodeError as failure:
        decoded = None
        error = str(failure)
    return {"enums": exporter.enums, "type": exported, "value": value, "decoded": decoded, "error": error}


def outputs(case: dict[str, Any]) -> list[tuple[str, object]]:
    found: list[tuple[str, object]] = []
    for entry in case.get("responses", []):
        found.extend((entry["capability"], result["output"]) for result in entry["results"] if "output" in result)
    requested = {item["request"]: item["capability"] for item in case.get("trace", []) if "request" in item}
    for step in case.get("steps", []):
        for request_id, entry in step["respond"]:
            if request_id in requested and "output" in entry:
                found.append((requested[request_id], entry["output"]))
    return found


def response(entry: dict[str, Any]) -> Response:
    if "output" in entry:
        return Output(entry["output"])
    return Refusal(entry["error"]["kind"], entry["error"]["message"])


def outcome(case: dict[str, Any], program: ir.Program, registry: Registry, budgets: Budgets) -> dict[str, Any]:
    finished: Outcome
    if "steps" in case:
        machine = Machine(program, budgets=budgets)
        finished = machine.start()
        for step in case["steps"]:
            finished = machine.resume([(request_id, response(entry)) for request_id, entry in step["respond"]])
    else:
        finished = run(program, FixtureHost.from_responses(registry, case["responses"]), budgets=budgets).outcome
    if isinstance(finished, Completed):
        return {"completed": finished.json, "shown": show_value(finished.value)}
    if isinstance(finished, Suspended):
        return {"suspended": [request.id for request in finished.requests]}
    assert isinstance(finished, Failed)
    failure = finished.failure
    span = failure.span
    where = None if span is None else [span.start.line, span.start.column, span.end.line, span.end.column]
    return {"failed": failure.kind.value, "message": failure.message, "span": where, "request": failure.request}


def main(target: Path) -> int:
    target.mkdir(parents=True, exist_ok=True)
    values: list[dict[str, Any]] = []
    for path in sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json"):
        case = json.loads(path.read_text())
        if case.get("python") is False:
            continue
        registry = Registry()
        load_declarations(registry, json.loads((FIXTURES / case["declarations"]).read_text()))
        budgets = Budgets(**case.get("budgets", {}))
        checked = compile_source(case["program"], registry, allowed=frozenset(case["allow"]), budgets=budgets)
        for capability_id, output in outputs(case):
            capability = registry.capability(capability_id)
            if capability is not None:
                values.append(value_case(capability.output, output))
        if checked.program is not None:
            (target / f"{path.stem}.ir.json").write_text(json.dumps(export_program(checked.program, {})))
            ran = outcome(case, checked.program, registry, budgets)
            (target / f"{path.stem}.outcome.json").write_text(json.dumps(ran))
    (target / "values.json").write_text(json.dumps(values))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(Path(sys.argv[1])))
