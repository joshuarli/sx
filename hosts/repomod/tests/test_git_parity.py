from itertools import product
from pathlib import Path

from evaluate import evaluator
from repomod.cmd.status import _age_days, _local_label, _stale_local_work
from repomod.types import GitStatus
from sx_repomod.capabilities import checkout_json

CASE = "Record {checkout: git.Checkout, default: Option<String>, branch: Option<Int>, unpushed: Option<Int>}"
PATH = Path("/unused")


def optional(days: int | None) -> str:
    return "none" if days is None else f"(some {days})"


def statuses() -> list[GitStatus]:
    shapes: list[tuple[bool, bool, bool, bool, str | None, str | None]] = [
        (False, False, False, False, None, None),
        (True, False, False, False, None, "directory is not a valid Git checkout"),
        (True, True, False, False, "main", "origin does not match the catalogued GitHub repository"),
        (True, True, False, True, None, "origin does not match the catalogued GitHub repository"),
        (True, True, True, True, None, None),
        (True, True, True, False, "main", None),
        (True, True, True, False, "topic", None),
    ]
    counts = [(0, 0, 0), (1, 0, 0), (0, 2, 1)]
    tracking = [(None, None), (0, 0), (2, 0), (0, 3), (1, 1), (2, None), (None, 1)]
    found = []
    for (present, valid, origin, detached, branch, error), (staged, unstaged, untracked), (ahead, behind) in product(
        shapes, counts, tracking
    ):
        found.append(
            GitStatus(
                "acme/tool", PATH, present, valid, branch, detached, staged, unstaged, untracked,
                ahead, behind, None if ahead is None else "origin/main", origin, error,
            )
        )
    return found


def test_local_label_matches_repomod_for_every_status_shape() -> None:
    cases = []
    expected = []
    for status in statuses():
        for default, branch_days, unpushed_days in product(("main", None), (None, 0, 5), (None, 0, 8)):
            cases.append(
                {
                    "checkout": checkout_json(status),
                    "default": default,
                    "branch": branch_days,
                    "unpushed": unpushed_days,
                }
            )
            expected.append(_local_label(status, default, branch_days, unpushed_days))
    source = (
        "(map (fn [c] (git.local-label (get c :checkout) (get c :default) (get c :branch) (get c :unpushed)))"
        " cases)"
    )
    with evaluator("git") as evaluate:
        labels = evaluate(source, {"cases": (f"List<{CASE}>", cases)})
    assert labels == expected
    assert len(set(expected)) > 15


def test_age_and_staleness_match_repomod() -> None:
    pairs = [
        (since, now)
        for since in (None, 0, 1, 86_399, 86_400, 1_700_000_000, 1_800_000_000, 1_800_000_500)
        for now in (1_800_000_000, 1_800_000_001, 1_800_086_399, 1_800_086_400, 1_900_000_000)
    ]
    cases = [{"since": since, "now": now * 1000} for since, now in pairs]
    source = "(map (fn [c] (git.age-days (get c :now) (get c :since))) cases)"
    with evaluator("git") as evaluate:
        ages = evaluate(source, {"cases": ("List<Record {since: Option<Int>, now: Int}>", cases)})
        assert ages == [_age_days(since, float(now)) for since, now in pairs]
        for branch, unpushed in product((None, 0, 6, 7, 30), repeat=2):
            source = f"(git.stale-work? {optional(branch)} {optional(unpushed)})"
            assert evaluate(source) == _stale_local_work(branch, unpushed)
