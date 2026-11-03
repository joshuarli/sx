from collections import Counter
from collections.abc import Iterable, Sequence
from dataclasses import dataclass

from records import AttemptStatus

STOPPED: frozenset[AttemptStatus] = frozenset({"rejected", "refused", "failed"})
UNCODED = "(no code)"


@dataclass(frozen=True, slots=True)
class Tried:
    status: AttemptStatus
    reasons: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class ReasonCount:
    reason: str
    programs: int
    repairs: int


def tally(sessions: Iterable[Sequence[Tried]]) -> list[ReasonCount]:
    programs: Counter[str] = Counter()
    repairs: Counter[str] = Counter()
    for tried in sessions:
        for position, each in enumerate(tried):
            if each.status not in STOPPED:
                continue
            repaired = position + 1 < len(tried)
            for reason in dict.fromkeys(each.reasons or (UNCODED,)):
                programs[reason] += 1
                repairs[reason] += repaired
    ranked = sorted(programs, key=lambda reason: (-repairs[reason], -programs[reason], reason))
    return [ReasonCount(reason, programs[reason], repairs[reason]) for reason in ranked]


def lines(counts: list[ReasonCount]) -> list[str]:
    rows = [
        "| Reason | Programs stopped | Repairs caused |",
        "| --- | --- | --- |",
    ]
    rows += [f"| {c.reason} | {c.programs} | {c.repairs} |" for c in counts]
    return rows if counts else ["No program was rejected, refused, or failed."]
