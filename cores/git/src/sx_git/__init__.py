from __future__ import annotations

from sx.registry import Assurance, Core, Evidence, Example, Function
from sx.types import BOOL, INT, STRING, EnumType, OptionType, RecordType, Type
from sx.values import NONE_VALUE, RecordValue, SomeValue, Value, VariantValue

STALE_LOCAL_WORK_DAYS = 7
MILLISECONDS_PER_DAY = 86_400_000

HEAD = RecordType(
    (
        ("staged", INT),
        ("unstaged", INT),
        ("untracked", INT),
        ("ahead", OptionType(INT)),
        ("behind", OptionType(INT)),
        ("upstream", OptionType(STRING)),
    ),
    name="git.Head",
)
CHECKOUT = EnumType(
    "git.Checkout",
    (
        ("Missing", None),
        ("Invalid", STRING),
        ("OriginMismatch", RecordType((("branch", OptionType(STRING)), ("head", HEAD), ("error", STRING)))),
        ("Detached", HEAD),
        ("OnBranch", RecordType((("branch", STRING), ("head", HEAD)))),
    ),
)
TYPES: dict[str, Type] = {"git.Head": HEAD, "git.Checkout": CHECKOUT}


def some(value: Value | None) -> Value:
    return NONE_VALUE if value is None else SomeValue(value)


def present(value: Value) -> Value | None:
    return value.value if isinstance(value, SomeValue) else None


def checkout(value: Value) -> VariantValue:
    assert isinstance(value, VariantValue)
    return value


def record(value: Value | None) -> RecordValue:
    assert isinstance(value, RecordValue)
    return value


def head_of(value: Value) -> RecordValue | None:
    variant = checkout(value)
    match variant.variant:
        case "Detached":
            return record(variant.payload)
        case "OriginMismatch" | "OnBranch":
            return record(record(variant.payload).get("head"))
        case _:
            return None


def branch_of(value: Value) -> str | None:
    variant = checkout(value)
    if variant.variant not in {"OriginMismatch", "OnBranch"}:
        return None
    branch = record(variant.payload).get("branch")
    if variant.variant == "OnBranch":
        assert isinstance(branch, str)
        return branch
    inner = present(branch)
    assert inner is None or isinstance(inner, str)
    return inner


def counts(head: RecordValue) -> int:
    staged, unstaged, untracked = head.get("staged"), head.get("unstaged"), head.get("untracked")
    assert isinstance(staged, int)
    assert isinstance(unstaged, int)
    assert isinstance(untracked, int)
    return staged + unstaged + untracked


def optional_int(value: Value) -> int | None:
    inner = present(value)
    assert inner is None or isinstance(inner, int)
    return inner


def local_label(value: Value, default_branch: Value, branch_days: Value, unpushed_days: Value) -> Value:
    variant = checkout(value)
    match variant.variant:
        case "Missing":
            return "missing"
        case "Invalid":
            return "invalid"
        case "OriginMismatch":
            return "invalid origin"
        case "Detached":
            return "detached"
    head, branch = head_of(value), branch_of(value)
    assert head is not None
    assert branch is not None
    ahead, behind = optional_int(head.get("ahead")), optional_int(head.get("behind"))
    changed = counts(head)
    if changed:
        state = f"dirty ({changed})"
    elif ahead is not None and behind is not None and ahead > 0 and behind > 0:
        state = f"diverged {ahead}/{behind} (last fetch)"
    elif ahead is not None and ahead > 0:
        state = f"ahead {ahead} (last fetch)"
    elif behind is not None and behind > 0:
        state = f"behind {behind} (last fetch)"
    else:
        state = "clean"
    unpushed = optional_int(unpushed_days)
    if unpushed is not None:
        state += f", oldest unpushed {unpushed}d"
    if present(default_branch) != branch:
        state += f" on {branch}"
        days = optional_int(branch_days)
        if days is not None:
            state += f" for {days}d"
    return state


def age_days(now_ms: Value, since_seconds: Value) -> Value:
    assert isinstance(now_ms, int)
    since = optional_int(since_seconds)
    return NONE_VALUE if since is None else SomeValue(max(0, (now_ms - since * 1000) // MILLISECONDS_PER_DAY))


def stale_work(branch_days: Value, unpushed_days: Value) -> Value:
    days = (optional_int(branch_days), optional_int(unpushed_days))
    return any(d is not None and d >= STALE_LOCAL_WORK_DAYS for d in days)


def error_of(value: Value) -> Value:
    variant = checkout(value)
    match variant.variant:
        case "Invalid":
            return SomeValue(variant.payload) if variant.payload is not None else NONE_VALUE
        case "OriginMismatch":
            return SomeValue(record(variant.payload).get("error"))
        case _:
            return NONE_VALUE


CORE = Core(
    id="git",
    version="0.1.0",
    types=TYPES,
    functions=(
        Function(
            "git.head", (CHECKOUT,), OptionType(HEAD),
            "The HEAD observation of a checkout, or none when the checkout is missing or invalid.",
            Example("(git.head Missing)", "none"),
            lambda c: some(head_of(c)),
        ),
        Function(
            "git.branch", (CHECKOUT,), OptionType(STRING),
            "The branch a checkout is on, or none when it has no HEAD or HEAD is detached.",
            Example("(git.branch Missing)", "none"),
            lambda c: some(branch_of(c)),
        ),
        Function(
            "git.error", (CHECKOUT,), OptionType(STRING),
            "Why a checkout is invalid or has the wrong origin, or none when nothing is wrong.",
            Example("(git.error Missing)", "none"),
            error_of,
        ),
        Function(
            "git.local-label", (CHECKOUT, OptionType(STRING), OptionType(INT), OptionType(INT)), STRING,
            "Summarizes a checkout, given the default branch, the days on this branch, and the days "
            "since the oldest unpushed commit.",
            Example("(git.local-label Missing none none none)", '"missing"'),
            local_label,
        ),
        Function(
            "git.age-days", (INT, OptionType(INT)), OptionType(INT),
            "Whole days from a Unix time in seconds to now in milliseconds, never negative, or none when "
            "the time is unknown.",
            Example("(git.age-days 172800000 (some 0))", "(some 2)"),
            age_days,
        ),
        Function(
            "git.stale-work?", (OptionType(INT), OptionType(INT)), BOOL,
            f"True when either age is known and at least {STALE_LOCAL_WORK_DAYS} days.",
            Example("(git.stale-work? (some 3) (some 7))", "true"),
            stale_work,
        ),
    ),
    evidence=(
        Evidence(
            Assurance.CONFORMANCE_TESTED,
            (
                "cores/git/tests/test_git_core.py::test_a_missing_observation_is_never_read_as_clean",
                "cores/git/tests/test_git_core.py::test_every_example_produces_its_documented_result",
            ),
        ),
    ),
)
