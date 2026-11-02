import re

import pytest

from sx import help as sx_help
from sx.checker import PRELUDE, SPECIAL_FORMS
from sx.host import FixtureHost, run
from sx.machine import Completed
from sx.program import compile_source
from sx.values import show_value

GUIDE_EXAMPLE = re.compile(r"```sx\n(.*?)```\n\nResult: `(.*?)`\n", re.DOTALL)


def evaluate(source: str) -> str:
    registry = sx_help.example_registry()
    allowed = frozenset(registry.capabilities)
    checked = compile_source(source, registry, allowed=allowed)
    assert checked.program is not None, [d.message for d in checked.diagnostics]
    outcome = run(checked.program, FixtureHost.from_responses(registry, sx_help.EXAMPLE_RESPONSES)).outcome
    assert isinstance(outcome, Completed), outcome
    return show_value(outcome.value)


@pytest.mark.parametrize("entry", sx_help.ENTRIES.values(), ids=list(sx_help.ENTRIES))
def test_every_help_example_produces_its_documented_result(entry: sx_help.Entry) -> None:
    assert evaluate(entry.example) == entry.result


def test_every_guide_example_produces_its_documented_result() -> None:
    examples = GUIDE_EXAMPLE.findall(sx_help.guide())
    assert len(examples) == 3
    for source, result in examples:
        assert evaluate(source) == result


def test_every_form_and_prelude_function_has_a_help_entry() -> None:
    assert sorted((SPECIAL_FORMS | PRELUDE | {"none"}) - set(sx_help.ENTRIES)) == []


def test_the_guide_names_every_form_and_prelude_function() -> None:
    text = sx_help.guide()
    missing = [name for name in sorted(SPECIAL_FORMS | PRELUDE) if f"({name} " not in text and f"`{name}`" not in text]
    assert missing == []
