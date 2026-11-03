from collections import defaultdict
from pathlib import Path
from statistics import mean

import gate
import repairs
from probe import Trial, load_trials
from records import AttemptStatus

ARM_ORDER = ("sx", "js", "ts", "jsbody")
CANDIDATE = "ts"
BASELINE = "jsbody"
ALL_SETS = "all sets"
STOP_STATUSES: tuple[AttemptStatus, ...] = ("rejected", "failed")


def load(runs: list[Path]) -> list[Trial]:
    trials = [trial for run in runs for trial in load_trials(run)]
    if not trials:
        raise FileNotFoundError(f"no trial.json under {', '.join(map(str, runs))}")
    return trials


def scored(trials: list[Trial]) -> list[gate.Scored]:
    return [gate.Scored(t.model, t.arm, t.final_correct, t.cost) for t in trials]


def summary_rows(trials: list[Trial]) -> list[str]:
    rows = [
        "| Set | Model | Arm | Trials | Correct within 3 attempts | Correct at attempt 1 | Mean cost per trial "
        "| Mean tool calls of attempts not ending correct |",
        "| --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    sets = sorted({t.workflow_set for t in trials})
    models = sorted({t.model for t in trials})
    groups: list[tuple[str, str, list[Trial]]] = []
    for workflow_set in [*sets, ALL_SETS]:
        in_set = [t for t in trials if workflow_set in (ALL_SETS, t.workflow_set)]
        groups += [(workflow_set, model, [t for t in in_set if t.model == model]) for model in models]
        groups.append((workflow_set, gate.POOLED, in_set))
    for workflow_set, model, members in groups:
        for arm in ARM_ORDER:
            mine = [t for t in members if t.arm == arm]
            if not mine:
                continue
            final, first = sum(t.final_correct for t in mine), sum(t.first_correct for t in mine)
            total = len(mine)
            rows.append(
                f"| {workflow_set} | {model} | {arm} | {total} | {final}/{total} ({100 * final / total:.1f}%) "
                f"| {first}/{total} ({100 * first / total:.1f}%) | ${mean(t.cost for t in mine):.4f} "
                f"| {mean(t.calls_without_success for t in mine):.2f} |"
            )
    return rows


def gate_sections(trials: list[Trial], candidate: str, baseline: str) -> list[str]:
    sections = ["### All sets", "", *gate.lines(scored(trials), ARM_ORDER, candidate, baseline), ""]
    for workflow_set in sorted({t.workflow_set for t in trials}):
        subset = [t for t in trials if t.workflow_set == workflow_set]
        sections += [f"### {workflow_set}", "", *gate.lines(scored(subset), ARM_ORDER, candidate, baseline), ""]
    return sections


def stops(trials: list[Trial], arm: str) -> list[str]:
    series = [
        [
            repairs.Tried(a.status if a.status in STOP_STATUSES else "completed", tuple(a.diagnostics))
            for a in t.attempts
        ]
        for t in trials
        if t.arm == arm
    ]
    return repairs.lines(repairs.tally(series))


def attempts_by_status(trials: list[Trial], arm: str) -> list[str]:
    counts: dict[str, int] = defaultdict(int)
    for t in trials:
        if t.arm == arm:
            for a in t.attempts:
                counts[a.status] += 1
    shown = ", ".join(f"{status} {count}" for status, count in sorted(counts.items())) or "none"
    return [f"Attempts of the {arm} arm by status: {shown}."]


def render(runs: list[Path], candidate: str = CANDIDATE, baseline: str = BASELINE) -> str:
    trials = load(runs)
    return "\n".join([
        "## Correctness, cost, and wasted tool calls", "", *summary_rows(trials), "",
        f"## Gate: {candidate} against {baseline}", "", *gate_sections(trials, candidate, baseline),
        f"## Where the {candidate} arm stopped, by diagnostic code", "", *stops(trials, candidate), "",
        *attempts_by_status(trials, candidate), "",
    ])
