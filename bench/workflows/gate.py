from collections import defaultdict
from dataclasses import dataclass

CORRECTNESS_MARGIN_POINTS = 5.0
COST_RATIO_LIMIT = 1.25
CANDIDATE = "ts"
BASELINE = "js"
POOLED = "all models"


@dataclass(frozen=True, slots=True)
class Scored:
    model: str
    arm: str
    correct: bool
    cost: float


@dataclass(frozen=True, slots=True)
class Cell:
    sessions: int
    correct: int
    cost: float

    @property
    def correctness(self) -> float:
        return 100 * self.correct / self.sessions

    @property
    def mean_cost(self) -> float:
        return self.cost / self.sessions


def cells(scored: list[Scored]) -> dict[tuple[str, str], Cell]:
    grouped: dict[tuple[str, str], list[Scored]] = defaultdict(list)
    for each in scored:
        grouped[each.model, each.arm].append(each)
        grouped[POOLED, each.arm].append(each)
    return {
        key: Cell(len(members), sum(m.correct for m in members), sum(m.cost for m in members))
        for key, members in grouped.items()
    }


def verdict(candidate: Cell, baseline: Cell) -> tuple[float, float | None, bool]:
    gap = candidate.correctness - baseline.correctness
    ratio = candidate.mean_cost / baseline.mean_cost if baseline.mean_cost else None
    passed = gap >= -CORRECTNESS_MARGIN_POINTS and ratio is not None and ratio <= COST_RATIO_LIMIT
    return gap, ratio, passed


def lines(
    scored: list[Scored], arm_order: tuple[str, ...], candidate_arm: str = CANDIDATE, baseline_arm: str = BASELINE,
) -> list[str]:
    found = cells(scored)
    models = sorted({s.model for s in scored})
    rows = [
        "| Model | Arm | Sessions | Correct | Correctness | Mean cost per session |",
        "| --- | --- | --- | --- | --- | --- |",
    ]
    for model in [*models, POOLED]:
        for arm in arm_order:
            cell = found.get((model, arm))
            if cell is not None:
                rows.append(
                    f"| {model} | {arm} | {cell.sessions} | {cell.correct}/{cell.sessions} "
                    f"| {cell.correctness:.1f}% | ${cell.mean_cost:.4f} |"
                )
    gate = [
        "",
        f"| Model | {candidate_arm} minus {baseline_arm} correctness (points) | {candidate_arm} / {baseline_arm} cost "
        f"| Gate (within {CORRECTNESS_MARGIN_POINTS:.0f} points, at most {COST_RATIO_LIMIT}x) |",
        "| --- | --- | --- | --- |",
    ]
    for model in [*models, POOLED]:
        candidate, baseline = found.get((model, candidate_arm)), found.get((model, baseline_arm))
        if candidate is None or baseline is None:
            continue
        gap, ratio, passed = verdict(candidate, baseline)
        shown = "n/a" if ratio is None else f"{ratio:.2f}x"
        gate.append(f"| {model} | {gap:+.1f} | {shown} | {'pass' if passed else 'fail'} |")
    return [*rows, *gate]
