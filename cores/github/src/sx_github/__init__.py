from __future__ import annotations

from sx.registry import Assurance, Core, Evidence, Example, Function
from sx.types import BOOL, INT, STRING, EnumType, ListType, OptionType, RecordType, Type
from sx.values import ListValue, RecordValue, SomeValue, Value, VariantValue

CI_STATES = ("Passed", "Failed", "Pending", "Cancelled", "Skipped", "Neutral", "NoChecks", "Unknown", "Stale")
UPSTREAM_STATES = ("Identical", "Ahead", "Behind", "Diverged", "Unknown")

CI_STATE = EnumType("github.CiState", tuple((name, None) for name in CI_STATES))
UPSTREAM_STATE = EnumType("github.UpstreamState", tuple((name, None) for name in UPSTREAM_STATES))
CHECK = RecordType(
    (
        ("name", STRING),
        ("state", STRING),
        ("source", OptionType(STRING)),
        ("updated-at", OptionType(STRING)),
        ("suite-id", OptionType(STRING)),
        ("run-id", OptionType(STRING)),
        ("attempt", OptionType(INT)),
    ),
    name="github.Check",
)
WORKFLOW = RecordType(
    (("name", STRING), ("state", CI_STATE), ("url", OptionType(STRING))),
    name="github.Workflow",
)
CI_REPORT = RecordType(
    (
        ("sha", OptionType(STRING)),
        ("default-branch", OptionType(STRING)),
        ("state", CI_STATE),
        ("age-seconds", INT),
        ("workflows", OptionType(ListType(WORKFLOW))),
    ),
    name="github.CiReport",
)
CI_CACHE = EnumType("github.CiCache", (("NotCached", STRING), ("Cached", CI_REPORT)))
UPSTREAM = RecordType(
    (
        ("parent", STRING),
        ("parent-branch", OptionType(STRING)),
        ("parent-sha", OptionType(STRING)),
        ("behind-by", INT),
        ("ahead-by", INT),
        ("state", UPSTREAM_STATE),
        ("age-seconds", INT),
    ),
    name="github.Upstream",
)
TYPES: dict[str, Type] = {
    "github.CiState": CI_STATE,
    "github.UpstreamState": UPSTREAM_STATE,
    "github.Check": CHECK,
    "github.Workflow": WORKFLOW,
    "github.CiReport": CI_REPORT,
    "github.CiCache": CI_CACHE,
    "github.Upstream": UPSTREAM,
}

FAILURE_STATES = frozenset({"failure", "failed", "error", "timed_out", "action_required", "startup_failure"})
PENDING_STATES = frozenset({"queued", "requested", "waiting", "pending", "expected", "in_progress"})
SEVERITY = {**dict.fromkeys(FAILURE_STATES, 5), **dict.fromkeys(PENDING_STATES, 4), "cancelled": 3}


class Check:
    def __init__(self, value: Value) -> None:
        assert isinstance(value, RecordValue)
        self.name = text(value.get("name"))
        self.state = text(value.get("state"))
        self.source = optional_text(value.get("source"))
        self.updated_at = optional_text(value.get("updated-at"))
        self.suite_id = optional_text(value.get("suite-id"))
        self.run_id = optional_text(value.get("run-id"))
        attempt = value.get("attempt")
        inner = attempt.value if isinstance(attempt, SomeValue) else None
        assert inner is None or isinstance(inner, int)
        self.attempt = inner

    @property
    def run_identity(self) -> str | None:
        return self.run_id or self.suite_id


def text(value: Value) -> str:
    assert isinstance(value, str)
    return value


def optional_text(value: Value) -> str | None:
    inner = value.value if isinstance(value, SomeValue) else None
    assert inner is None or isinstance(inner, str)
    return inner


def aggregate(checks_value: Value, complete: Value) -> Value:
    if not complete:
        return VariantValue("github.CiState", "Unknown")
    assert isinstance(checks_value, ListValue)
    checks = [Check(item) for item in checks_value.items]
    attempts: dict[str, int] = {}
    for check in checks:
        identity = check.run_identity
        if check.suite_id is not None and check.attempt is not None and identity is not None:
            attempts[identity] = max(check.attempt, attempts.get(identity, 0))
    latest_statuses: dict[tuple[str, str], Check] = {}
    independent: list[Check] = []
    for check in checks:
        if check.source is not None and check.source.startswith("status:"):
            if check.updated_at is None:
                independent.append(check)
                continue
            key = (check.name.casefold(), check.source.casefold())
            previous = latest_statuses.get(key)
            newer = previous is None or previous.updated_at is None or check.updated_at > previous.updated_at
            same_time_failure = (
                previous is not None
                and check.updated_at == previous.updated_at
                and SEVERITY.get(check.state.casefold(), 0) > SEVERITY.get(previous.state.casefold(), 0)
            )
            if newer or same_time_failure:
                latest_statuses[key] = check
            continue
        identity = check.run_identity
        if (
            check.suite_id is not None
            and check.attempt is not None
            and identity is not None
            and check.attempt < attempts[identity]
        ):
            continue
        independent.append(check)
    current = (*latest_statuses.values(), *independent)
    return VariantValue("github.CiState", ci_variant(current))


def ci_variant(current: tuple[Check, ...]) -> str:
    if not current:
        return "NoChecks"
    states = {check.state.casefold() for check in current}
    if states & FAILURE_STATES:
        return "Failed"
    if states & PENDING_STATES:
        return "Pending"
    if "cancelled" in states:
        return "Cancelled"
    if states <= {"skipped", "neutral"}:
        return "Neutral" if states == {"neutral"} else "Skipped"
    if states <= {"success", "passed", "neutral", "skipped"}:
        return "Passed"
    if "stale" in states:
        return "Stale"
    return "Unknown"


def variant_name(value: Value) -> str:
    assert isinstance(value, VariantValue)
    return value.variant


CI_LABELS = {name: "none" if name == "NoChecks" else name.lower() for name in CI_STATES}
CI_VARIANTS = {label: name for name, label in CI_LABELS.items()}


def ci_state_name(value: Value) -> Value:
    return CI_LABELS[variant_name(value)]


def parse_ci_state(label: Value) -> Value:
    assert isinstance(label, str)
    return VariantValue("github.CiState", CI_VARIANTS.get(label, "Unknown"))


CORE = Core(
    id="github",
    version="0.1.0",
    types=TYPES,
    functions=(
        Function(
            "github.ci-state", (ListType(CHECK), BOOL), CI_STATE,
            "Aggregates a commit's checks into one state. Reruns replace earlier attempts, a newer external "
            "status replaces an older one from the same creator, and an incomplete listing is Unknown.",
            Example("(github.ci-state [] true)", "NoChecks"),
            aggregate,
        ),
        Function(
            "github.ci-state-name", (CI_STATE,), STRING,
            "The lowercase name of a CI state, with none for NoChecks.",
            Example("(github.ci-state-name NoChecks)", '"none"'),
            ci_state_name,
        ),
        Function(
            "github.parse-ci-state", (STRING,), CI_STATE,
            "Reads a lowercase CI state name. A name that is no state is Unknown, never Passed.",
            Example('(github.parse-ci-state "passed")', "Passed"),
            parse_ci_state,
        ),
        Function(
            "github.upstream-state-name", (UPSTREAM_STATE,), STRING,
            "The lowercase name of an upstream comparison state.",
            Example("(github.upstream-state-name Diverged)", '"diverged"'),
            lambda state: variant_name(state).lower(),
        ),
    ),
    evidence=(
        Evidence(
            Assurance.CONFORMANCE_TESTED,
            (
                "cores/github/tests/test_github_core.py::test_aggregation_cases_from_repomod",
                "cores/github/tests/test_github_core.py::test_every_example_produces_its_documented_result",
                "tests/test_core_vectors.py::test_committed_vectors_match_the_python_core",
                "hosts/repomod/tests/test_github_parity.py::test_ci_state_matches_repomod_for_generated_check_sets",
                "hosts/repomod/tests/test_status_parity.py::test_status_offline_json_matches_python_on_every_fixture",
            ),
        ),
    ),
)
