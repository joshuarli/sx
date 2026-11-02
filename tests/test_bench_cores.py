import json
from pathlib import Path
from typing import Any

import pytest
from cores import core

from sx.host import FixtureHost, run
from sx.machine import Completed
from sx.program import compile_source
from sx.registry import Registry
from sx.values import show_value

BENCH = Path(__file__).parent.parent / "bench" / "codemode"
FUNCTIONS = json.loads((BENCH / "cores.json").read_text())["functions"]


@pytest.mark.parametrize("function", FUNCTIONS, ids=[f["name"] for f in FUNCTIONS])
def test_every_core_example_produces_its_documented_result(function: dict[str, Any]) -> None:
    registry = Registry()
    registry.add_core(core(str(function["name"]).split(".")[0], registry))
    program, result = function["example"]
    assert isinstance(program, str)
    checked = compile_source(program, registry, allowed=frozenset())
    assert checked.program is not None, [d.message for d in checked.diagnostics]
    outcome = run(checked.program, FixtureHost(registry)).outcome
    assert isinstance(outcome, Completed)
    assert show_value(outcome.value) == result
