from __future__ import annotations

from collections.abc import Sequence
from typing import Any, cast

from repomod.cmd import common
from repomod.cmd.status import _status_text
from repomod.types import CiResult, CiState, Finding, WorkflowStatus

type Row = dict[str, Any]


def discover_json(result: Row) -> None:
    common._print_json(result)


def discover_text(result: Row) -> None:
    uncatalogued, missing = result["uncatalogued"], result["missing"]
    if uncatalogued:
        print(f"uncatalogued ({len(uncatalogued)}):")
        for name in uncatalogued:
            print(f"  {name}")
    else:
        print("uncatalogued (0): none")
    if missing:
        print(f"missing from GitHub ({len(missing)}):")
        for name in missing:
            print(f"  {name}")
    elif not uncatalogued:
        print("catalog matches GitHub")


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
    common._print_json(list(rows))


def status_text(rows: Sequence[Row]) -> None:
    findings = {str(row["repository"]): [finding(f) for f in row["lint_findings"]] for row in rows}
    _status_text(list(rows), findings, {str(row["repository"]): ci_result(row) for row in rows})
