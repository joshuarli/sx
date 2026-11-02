import sys

import pytest
from sx_text import CORE

from sx.checker import Budgets
from sx.host import FixtureHost, evaluate, run
from sx.program import compile_source
from sx.registry import Registry
from sx.types import STRING, ListType
from sx.values import ListValue, show_value

SURROGATES = range(0xD800, 0xE000)
CHUNK = 20_000
CODE_POINTS = [chr(cp) for cp in range(sys.maxunicode + 1) if cp not in SURROGATES]
TRICKY = ["ß", "İ", "ΑΣ", "ǅ", "ﬁ", "ŉ", "ẞ", "Straße", "ΧΑΟΣ", "ᾈ", "ͅ", "LaPuta"]


def registry() -> Registry:
    registry = Registry()
    registry.add_core(CORE)
    return registry


def casefold_all(texts: list[str]) -> list[str]:
    budgets = Budgets(steps=10_000_000, applications=1_000_000, value_size=10_000_000)
    inputs = {"texts": ListType(STRING)}
    checked = compile_source("(map text.casefold texts)", registry(), allowed=frozenset(), inputs=inputs)
    assert checked.program is not None
    outcome = run(checked.program, FixtureHost(registry()), {"texts": ListValue(tuple(texts))}, budgets).outcome
    value = getattr(outcome, "value", None)
    assert isinstance(value, ListValue)
    assert all(isinstance(item, str) for item in value.items)
    return [str(item) for item in value.items]


@pytest.mark.parametrize("start", range(0, len(CODE_POINTS), CHUNK))
def test_casefold_matches_python_for_every_code_point(start: int) -> None:
    chunk = CODE_POINTS[start : start + CHUNK]
    assert casefold_all(chunk) == [c.casefold() for c in chunk]


def test_casefold_matches_python_on_multi_character_expansions() -> None:
    assert casefold_all(TRICKY) == [t.casefold() for t in TRICKY]
    assert casefold_all(TRICKY) != [t.lower() for t in TRICKY]


def test_every_example_produces_its_documented_result() -> None:
    for function in CORE.functions:
        assert function.example is not None
        assert show_value(evaluate(registry(), function.example.program)) == function.example.result
