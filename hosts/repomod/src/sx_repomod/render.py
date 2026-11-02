from __future__ import annotations

from collections.abc import Sequence
from typing import Any, cast

from repomod.cmd import common
from repomod.cmd.status import status_text as repomod_status_text
from repomod.types import CiResult, CiState, Finding, WorkflowStatus

type Row = dict[str, Any]


def finding(record: Row) -> Finding:
    return Finding(**record)


def ci_result(row: Row) -> CiResult:
    workflows = row["workflows"]
    return CiResult(
        row["repository"],
        row["default_branch"],
        row["ci_sha"],
        cast(CiState, row["ci_state"]),
        (),
        cached=row["ci_cached"],
        age_seconds=row["ci_age_seconds"],
        error=row["ci_error"],
        workflows=None
        if workflows is None
        else tuple(WorkflowStatus(w["name"], w["state"], w["url"]) for w in workflows),
    )


def status_json(rows: Sequence[Row]) -> None:
    common.print_json(list(rows))


def status_text(rows: Sequence[Row]) -> None:
    findings = {str(row["repository"]): [finding(f) for f in row["lint_findings"]] for row in rows}
    repomod_status_text(list(rows), findings, {str(row["repository"]): ci_result(row) for row in rows})
