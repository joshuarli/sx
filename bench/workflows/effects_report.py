from pathlib import Path

import gate
import repairs
from metrics import Measure, Totals, totals
from records import CODE_SERVER_ARMS, SESSION_ARMS
from session import SessionRecord, load_record

ARM_ORDER = SESSION_ARMS


def load_records(out: Path) -> list[SessionRecord]:
    return [load_record(path.parent) for path in sorted(out.glob("*/*/session.json"))]


def share(value: float | None) -> str:
    return "n/a" if value is None else f"{value:.0%}"


def per_session(total: int, sessions: int) -> str:
    return f"{total / sessions:,.1f}"


def row(workflow: str, arm: str, records: list[SessionRecord], measures: list[Measure]) -> str:
    t: Totals = totals(measures)
    errored = sum(r.error is not None for r in records)
    cells = [
        workflow, arm, str(t.sessions), f"{t.correct}/{t.sessions}", str(t.violations),
        f"{t.violating_sessions}/{t.sessions}", str(t.partial_effects), share(t.stopped_share),
        per_session(t.attempts, t.sessions), per_session(t.executed, t.sessions), per_session(t.turns, t.sessions),
        per_session(t.input, t.sessions), per_session(t.output, t.sessions), per_session(t.cache_read, t.sessions),
        f"${t.cost / t.sessions:.4f}", str(errored),
    ]
    return "| " + " | ".join(cells) + " |"


def table(records: list[SessionRecord]) -> list[str]:
    lines = [
        "| Workflow | Arm | Sessions | Correct | Violating effects | Sessions with a violation | Partial effects "
        "| Stopped share | Attempts | Programs run | Turns | Input tokens | Output tokens | Cache read tokens "
        "| Cost | Errored |",
        "| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |",
    ]
    for workflow in sorted({r.workflow for r in records}):
        for arm in ARM_ORDER:
            mine = [r for r in records if r.workflow == workflow and r.arm == arm]
            if mine:
                lines.append(row(workflow, arm, mine, [r.measure() for r in mine]))
    return lines


def sessions(records: list[SessionRecord]) -> list[str]:
    lines = [
        "| Workflow | Arm | Sample | Attempts | Verdict | Violations | Error |",
        "| --- | --- | --- | --- | --- | --- | --- |",
    ]
    for r in sorted(records, key=lambda r: (r.workflow, r.arm, r.sample)):
        statuses = ", ".join(f"{a.tool}:{a.status}" for a in r.attempts) or "none"
        verdict = r.verdict.status if not r.verdict.detail else f"{r.verdict.status}: {r.verdict.detail}"
        violations = "; ".join(v.detail for v in r.violations) or "none"
        cells = [r.workflow, r.arm, str(r.sample), statuses, verdict, violations, r.error or ""]
        lines.append("| " + " | ".join(cells) + " |")
    return lines


def scored(records: list[SessionRecord]) -> list[gate.Scored]:
    return [gate.Scored(r.model, r.arm, r.verdict.status == "correct", r.usage.cost) for r in records]


def repair_lines(records: list[SessionRecord], arm: str) -> list[str]:
    sessions = [
        [repairs.Tried(a.status, (*a.diagnostics, *a.refusals)) for a in r.attempts]
        for r in records if r.arm == arm
    ]
    return repairs.lines(repairs.tally(sessions))


def render(out: Path) -> str:
    records = load_records(out)
    if not records:
        raise FileNotFoundError(f"no session.json under {out}")
    sections = [
        "## Gate: correctness and cost per session", "", *gate.lines(scored(records), ARM_ORDER), "",
        "## Effects per arm and workflow", "", *table(records), "",
    ]
    for arm in ARM_ORDER:
        if arm in CODE_SERVER_ARMS and any(r.arm == arm for r in records):
            sections += [f"## Programs stopped in the {arm} arm, by diagnostic, refusal, or trap", "",
                         *repair_lines(records, arm), ""]
    return "\n".join([*sections, "## Sessions", "", *sessions(records), ""])
