from dataclasses import dataclass
from typing import Any, Literal

type Json = dict[str, Any]
type EffectClass = Literal["read", "write"]
type CallKind = Literal[
    "Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed"
]
type Outcome = Literal["ok", "protocol-error", "unanswered"] | CallKind
type Tool = Literal["check", "run", "codemode"]
type AttemptStatus = Literal["checked", "rejected", "refused", "completed", "failed"]

CALL_KINDS: tuple[CallKind, ...] = (
    "Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed",
)
REFUSAL_KINDS: frozenset[CallKind] = frozenset({"RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict"})
OUTCOMES: tuple[Outcome, ...] = ("ok", "protocol-error", "unanswered", *CALL_KINDS)
TOOLS: tuple[Tool, ...] = ("check", "run", "codemode")
ATTEMPT_STATUSES: tuple[AttemptStatus, ...] = ("checked", "rejected", "refused", "completed", "failed")
EFFECT_CLASSES: tuple[EffectClass, ...] = ("read", "write")
NOT_PERFORMED: frozenset[Outcome] = frozenset({*REFUSAL_KINDS, "protocol-error"})
SAFETY_CODES = frozenset({"unauthorized", "out-of-scope", "retry-write", "parallel-write"})
SAFETY_REFUSALS: frozenset[Outcome] = frozenset({"PermissionDenied", "Conflict"})


def narrowed[T](value: object, options: tuple[T, ...]) -> T:
    for option in options:
        if option == value:
            return option
    raise ValueError(f"{value!r} is none of {options}")


@dataclass(frozen=True, slots=True)
class Effect:
    attempt: int
    capability: str
    input: Json
    outcome: Outcome
    effect_class: EffectClass
    time_ns: int

    @property
    def performed(self) -> bool:
        return self.effect_class == "write" and self.outcome not in NOT_PERFORMED


@dataclass(frozen=True, slots=True)
class Attempt:
    index: int
    tool: Tool
    program: str
    status: AttemptStatus
    diagnostics: tuple[str, ...] = ()
    refusals: tuple[str, ...] = ()
    started_ns: int = 0
    ended_ns: int = 0


@dataclass(frozen=True, slots=True)
class Usage:
    turns: int
    input: int
    output: int
    cache_read: int
    cost: float
