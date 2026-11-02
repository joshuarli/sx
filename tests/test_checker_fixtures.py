import json
from pathlib import Path
from typing import cast

import pytest

from sx.checker import check
from sx.diagnostics import render
from sx.reader import read
from sx.registry import Registry, declared_registry

ROOT = Path(__file__).parent.parent
FIXTURES = ROOT / "fixtures" / "check"
BENCH = ROOT / "bench" / "codemode"
PROGRAMS = sorted(FIXTURES.glob("*.sx"))
TASK_HEADER = "; task: "


def task_registry(task_id: str) -> tuple[Registry, frozenset[str]]:
    task = json.loads((BENCH / "tasks" / task_id / "task.json").read_text())
    cores = json.loads((BENCH / "cores.json").read_text())
    capability_ids = frozenset(cast(str, capability["id"]) for capability in task["capabilities"])
    return declared_registry(task, cores), capability_ids


@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_checker_fixture(program: Path) -> None:
    source = program.read_text()
    first_line = source.split("\n", 1)[0]
    assert first_line.startswith(TASK_HEADER), f"a checker fixture starts with {TASK_HEADER!r}"
    registry, allowed = task_registry(first_line.removeprefix(TASK_HEADER))
    result = read(source)
    assert result.program is not None, render(source, result.diagnostics)
    checked = check(result.program, registry, allowed=allowed)
    assert render(source, checked.diagnostics) == program.with_suffix(".expected").read_text()
