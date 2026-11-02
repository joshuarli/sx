from sx_semver import CORE

from sx.host import evaluate
from sx.registry import Registry
from sx.values import show_value


def registry() -> Registry:
    registry = Registry()
    registry.add_core(CORE)
    return registry


def test_compare_orders_versions_by_number_not_by_text() -> None:
    source = '(let [v (keep semver.parse ["1.10.0" "1.9.9"])] (semver.compare (nth v 0) (nth v 1)))'
    assert evaluate(registry(), source) == 1


def test_parse_rejects_anything_but_exact_numbers() -> None:
    for text in ("1.2", "1.2.3.4", "01.2.3", "v1.2.3", "1.2.-3", " 1.2.3", ""):
        assert show_value(evaluate(registry(), f'(semver.parse "{text}")')) == "none", text


def test_every_example_produces_its_documented_result() -> None:
    for function in CORE.functions:
        assert function.example is not None
        assert show_value(evaluate(registry(), function.example.program)) == function.example.result
