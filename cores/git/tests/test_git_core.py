import pytest
from sx_git import CHECKOUT, CORE

from sx.host import evaluate
from sx.registry import Registry
from sx.types import INT, STRING, OptionType, Type
from sx.values import NONE_VALUE, SomeValue, Value, decode, show_value


def head(**changes: object) -> dict[str, object]:
    base: dict[str, object] = {
        "staged": 0, "unstaged": 0, "untracked": 0, "ahead": None, "behind": None, "upstream": None,
    }
    return base | changes


def on_branch(branch: str, **changes: object) -> dict[str, object]:
    return {"tag": "OnBranch", "value": {"branch": branch, "head": head(**changes)}}


def optional(value: str | int | None) -> Value:
    return NONE_VALUE if value is None else SomeValue(value)


def run(source: str, checkout: object, **extra: tuple[Type, Value]) -> Value:
    registry = Registry()
    registry.add_core(CORE)
    return evaluate(registry, source, {"c": (CHECKOUT, decode(checkout, CHECKOUT, "$")), **extra})


def label(
    checkout: object, default: str | None = "main", branch_days: int | None = None, unpushed: int | None = None
) -> Value:
    return run(
        "(git.local-label c d b u)",
        checkout,
        d=(OptionType(STRING), optional(default)),
        b=(OptionType(INT), optional(branch_days)),
        u=(OptionType(INT), optional(unpushed)),
    )


def test_a_missing_observation_is_never_read_as_clean() -> None:
    assert show_value(run("(git.head c)", "Missing")) == "none"
    assert show_value(run("(git.branch c)", "Missing")) == "none"
    assert label("Missing") == "missing"
    assert label({"tag": "Invalid", "value": "not a checkout"}) == "invalid"
    assert show_value(run("(git.error c)", {"tag": "Invalid", "value": "not a checkout"})) == '(some "not a checkout")'


def test_only_checkouts_with_a_head_carry_one() -> None:
    mismatch = {"tag": "OriginMismatch", "value": {"branch": "main", "head": head(), "error": "wrong origin"}}
    detached = {"tag": "Detached", "value": head(untracked=2)}
    for checkout in (mismatch, detached, on_branch("main")):
        assert show_value(run("(if (git.head c) true false)", checkout)) == "true"
    assert show_value(run("(git.branch c)", detached)) == "none"
    assert show_value(run("(git.branch c)", mismatch)) == '(some "main")'
    assert show_value(run("(git.error c)", mismatch)) == '(some "wrong origin")'
    assert show_value(run("(git.error c)", on_branch("main"))) == "none"


@pytest.mark.parametrize(
    ("checkout", "default", "branch_days", "unpushed", "expected"),
    [
        (on_branch("main"), "main", None, None, "clean"),
        (on_branch("main", staged=1, unstaged=1, untracked=1), "main", None, None, "dirty (3)"),
        (on_branch("main", ahead=2, behind=0), "main", None, None, "ahead 2 (last fetch)"),
        (on_branch("main", ahead=0, behind=3), "main", None, None, "behind 3 (last fetch)"),
        (on_branch("main", ahead=1, behind=4), "main", None, None, "diverged 1/4 (last fetch)"),
        (on_branch("main", ahead=2, behind=0), "main", None, 9, "ahead 2 (last fetch), oldest unpushed 9d"),
        (on_branch("topic"), "main", 3, None, "clean on topic for 3d"),
        (on_branch("topic"), "main", None, None, "clean on topic"),
        (on_branch("topic"), None, None, None, "clean on topic"),
        ({"tag": "Detached", "value": head(staged=4)}, "main", 3, 3, "detached"),
        (
            {"tag": "OriginMismatch", "value": {"branch": None, "head": head(), "error": "x"}},
            "main", None, None, "invalid origin",
        ),
    ],
)
def test_label_follows_the_status_command(
    checkout: object, default: str, branch_days: int | None, unpushed: int | None, expected: str
) -> None:
    assert label(checkout, default, branch_days, unpushed) == expected


@pytest.mark.parametrize(
    ("now_ms", "since", "expected"),
    [(172_800_000, 0, "(some 2)"), (172_799_999, 0, "(some 1)"), (0, 5, "(some 0)"), (86_400_000, None, "none")],
)
def test_age_counts_whole_days_and_never_goes_negative(now_ms: int, since: int | None, expected: str) -> None:
    registry = Registry()
    registry.add_core(CORE)
    inputs = {"n": (INT, now_ms), "s": (OptionType(INT), optional(since))}
    assert show_value(evaluate(registry, "(git.age-days n s)", inputs)) == expected


def test_work_is_stale_from_seven_days_and_unknown_ages_are_not_stale() -> None:
    registry = Registry()
    registry.add_core(CORE)
    results = [
        show_value(evaluate(registry, f"(git.stale-work? {a} {b})"))
        for a, b in (("none", "none"), ("(some 6)", "(some 6)"), ("none", "(some 7)"), ("(some 7)", "none"))
    ]
    assert results == ["false", "false", "true", "true"]


def test_every_example_produces_its_documented_result() -> None:
    registry = Registry()
    registry.add_core(CORE)
    for function in CORE.functions:
        assert function.example is not None
        assert show_value(evaluate(registry, function.example.program)) == function.example.result
