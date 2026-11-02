import random

from evaluate import evaluator
from repomod.github import _state_for_checks
from repomod.types import CheckResult

STATES = [
    "success", "passed", "failure", "failed", "error", "timed_out", "action_required", "startup_failure",
    "queued", "requested", "waiting", "pending", "expected", "in_progress", "cancelled", "skipped",
    "neutral", "stale", "mystery", "FAILURE", "In_Progress",
]
SOURCES = [None, None, "GitHub Actions", "status:alpha", "status:Alpha", "status:beta", "STATUS:gamma"]
STAMPS = [None, "2026-10-20T12:00:00Z", "2026-10-21T12:00:00Z", "2026-10-21T12:01:00Z"]
SUITES = [None, "suite-a", "suite-b"]
RUNS = [None, None, "run-a", ""]
ATTEMPTS = [None, 1, 2, 3]
NAMES = ["build", "Build", "deploy", "scan"]
CASE = "Record {checks: List<github.Check>, complete: Bool}"


def generated(seed: int, count: int) -> list[tuple[list[CheckResult], bool]]:
    rng = random.Random(seed)
    cases = []
    for _ in range(count):
        checks = [
            CheckResult(
                rng.choice(NAMES), rng.choice(STATES), None, rng.choice(SOURCES), rng.choice(STAMPS),
                rng.choice(SUITES), rng.choice(RUNS), rng.choice(ATTEMPTS),
            )
            for _ in range(rng.randrange(0, 7))
        ]
        cases.append((checks, rng.random() < 0.9))
    return cases


def as_json(check: CheckResult) -> dict[str, object]:
    return {
        "name": check.name, "state": check.state, "source": check.source, "updated-at": check.updated_at,
        "suite-id": check.check_suite_id, "run-id": check.workflow_run_id, "attempt": check.run_attempt,
    }


def test_ci_state_matches_repomod_for_generated_check_sets() -> None:
    cases = generated(20261101, 4000)
    payload = [{"checks": [as_json(c) for c in checks], "complete": complete} for checks, complete in cases]
    source = "(map (fn [c] (github.ci-state-name (github.ci-state (get c :checks) (get c :complete)))) cases)"
    with evaluator("github") as evaluate:
        names = evaluate(source, {"cases": (f"List<{CASE}>", payload)})
    expected = [_state_for_checks(tuple(checks), complete) for checks, complete in cases]
    assert names == expected
    every_state = {"failed", "pending", "passed", "none", "unknown", "stale", "cancelled", "skipped", "neutral"}
    assert set(expected) == every_state
