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


@pytest.mark.parametrize("level", sx_help.GUIDE_LEVELS)
def test_every_guide_level_names_every_form_and_prelude_function(level: str) -> None:
    text = sx_help.guide(level)
    names = sorted(SPECIAL_FORMS | PRELUDE)
    spellings = ("({} ", "`{}`", " {} ")
    missing = [name for name in names if not any(form.format(name) in text for form in spellings)]
    assert missing == []


def test_guide_levels_grow_in_detail() -> None:
    sizes = [len(sx_help.guide(level)) for level in sx_help.GUIDE_LEVELS]
    assert sizes == sorted(sizes)
