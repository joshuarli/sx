import pytest
from sx_github import CHECK, CORE

from sx.host import evaluate
from sx.registry import Registry
from sx.types import BOOL, ListType
from sx.values import Value, decode, show_value


def check(
    name: str, state: str, source: str | None = None, updated_at: str | None = None,
    suite_id: str | None = None, run_id: str | None = None, attempt: int | None = None,
) -> dict[str, object]:
    return {
        "name": name, "state": state, "source": source, "updated-at": updated_at,
        "suite-id": suite_id, "run-id": run_id, "attempt": attempt,
    }


def registry() -> Registry:
    registry = Registry()
    registry.add_core(CORE)
    return registry


def aggregate(checks: list[dict[str, object]], complete: bool = True) -> Value:
    listed = decode(checks, ListType(CHECK), "$")
    inputs = {"checks": (ListType(CHECK), listed), "complete": (BOOL, complete)}
    return evaluate(registry(), "(github.ci-state-name (github.ci-state checks complete))", inputs)


ACTIONS = "GitHub Actions"
D20, D21 = "2026-10-20T12:00:00Z", "2026-10-21T12:00:00Z"
MINUTE_LATER = "2026-10-21T12:01:00Z"

CASES = [
    ([], True, "none"),
    ([], False, "unknown"),
    ([check("build", "failure")], True, "failed"),
    ([check("old success", "success"), check("rerun", "failure")], True, "failed"),
    ([check("build", "in_progress")], True, "pending"),
    ([check("build", "cancelled")], True, "cancelled"),
    ([check("workflow", "skipped")], True, "skipped"),
    ([check("external", "neutral")], True, "neutral"),
    ([check("build", "success")], True, "passed"),
    (
        [check("build", "failure", ACTIONS, D20, "suite-a", "run-a", 1),
         check("build", "success", ACTIONS, D21, "suite-b", "run-a", 2)],
        True, "passed",
    ),
    (
        [check("build", "failure", ACTIONS, D20, "suite-a", None, 1),
         check("build", "cancelled", ACTIONS, D21, "suite-a", None, 2)],
        True, "cancelled",
    ),
    (
        [check("build", "failure", ACTIONS, D21, "suite-a", None, 1),
         check("build", "success", ACTIONS, MINUTE_LATER, "suite-b", None, 1)],
        True, "failed",
    ),
    (
        [check("build", "failure", ACTIONS, None, "suite-a", None, 1),
         check("build", "in_progress", ACTIONS, None, "suite-a", None, 2),
         check("deploy preview", "success", "status:preview", D21),
         check("security scan", "failure", "status:scanner", D21)],
        True, "failed",
    ),
    (
        [check("build", "failure", ACTIONS, None, "suite-a", None, 1),
         check("build", "in_progress", ACTIONS, None, "suite-a", None, 2)],
        True, "pending",
    ),
    (
        [check("deploy", "failure", "status:preview-a", D21),
         check("deploy", "success", "status:preview-b", MINUTE_LATER)],
        True, "failed",
    ),
    ([check("lint", "stale")], True, "stale"),
    ([check("lint", "mystery")], True, "unknown"),
    ([check("lint", "success"), check("scan", "stale")], True, "stale"),
    (
        [check("deploy", "failure", "status:preview", D20), check("deploy", "success", "status:preview", D21)],
        True, "passed",
    ),
    (
        [check("deploy", "success", "status:preview", D21), check("deploy", "failure", "status:preview", D21)],
        True, "failed",
    ),
]


@pytest.mark.parametrize(("checks", "complete", "expected"), CASES, ids=[str(i) for i in range(len(CASES))])
def test_aggregation_cases_from_repomod(checks: list[dict[str, object]], complete: bool, expected: str) -> None:
    assert aggregate(checks, complete) == expected


def test_an_incomplete_listing_is_unknown_even_when_every_check_passed() -> None:
    assert aggregate([check("build", "success")], complete=False) == "unknown"


def test_a_state_name_that_is_no_state_is_unknown_never_passed() -> None:
    parsed = [
        evaluate(registry(), f'(github.ci-state-name (github.parse-ci-state "{name}"))')
        for name in ("passed", "none", "stale", "pending", "PASSED", "", "success")
    ]
    assert parsed == ["passed", "none", "stale", "pending", "unknown", "unknown", "unknown"]


def test_every_example_produces_its_documented_result() -> None:
    for function in CORE.functions:
        assert function.example is not None
        assert show_value(evaluate(registry(), function.example.program)) == function.example.result
