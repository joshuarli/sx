import json
from pathlib import Path
from typing import Any

import pytest

from sx.checker import Budgets
from sx.host import FixtureHost, run
from sx.machine import Completed, Failed, Machine, Outcome, Output, Refusal, Response, Suspended
from sx.program import compile_source
from sx.registry import Registry, load_declarations

FIXTURES = Path(__file__).parent.parent / "fixtures" / "run"
CASES = sorted(p for p in FIXTURES.glob("*.json") if p.name != "declarations.json")


def response(entry: dict[str, Any]) -> Response:
    if "output" in entry:
        return Output(entry["output"])
    return Refusal(entry["error"]["kind"], entry["error"]["message"])


def expected(outcome: Outcome) -> dict[str, object]:
    if isinstance(outcome, Completed):
        return {"completed": outcome.json}
    assert isinstance(outcome, Failed)
    return {"failed": outcome.failure.kind.value}


@pytest.mark.parametrize("path", CASES, ids=[p.stem for p in CASES])
def test_run_fixture(path: Path) -> None:
    case = json.loads(path.read_text())
    registry = Registry()
    load_declarations(registry, json.loads((FIXTURES / case["declarations"]).read_text()))
    budgets = Budgets(**case.get("budgets", {}))
    checked = compile_source(case["program"], registry, allowed=frozenset(case["allow"]), budgets=budgets)
    if "rejected" in case["expect"]:
        assert checked.program is None
        assert [d.code for d in checked.diagnostics] == case["expect"]["rejected"]
        return
    assert checked.program is not None, [d.message for d in checked.diagnostics]
    if "steps" in case:
        machine = Machine(checked.program, budgets=budgets)
        outcome = machine.start()
        for step in case["steps"]:
            assert isinstance(outcome, Suspended)
            assert [r.id for r in outcome.requests] == step["requests"]
            outcome = machine.resume([(request_id, response(entry)) for request_id, entry in step["respond"]])
    else:
        finished = run(checked.program, FixtureHost.from_responses(registry, case["responses"]), budgets=budgets)
        machine, outcome = finished.machine, finished.outcome
    assert expected(outcome) == case["expect"]
    assert machine.trace == case["trace"]
