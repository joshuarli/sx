import importlib
import json
from pathlib import Path

import pytest

from sx.diagnostics import render
from sx.reader import read

ROOT = Path(__file__).parent.parent
FIXTURES = ROOT / "fixtures" / "check"
TASKS = ROOT / "bench" / "codemode" / "tasks"
PROGRAMS = sorted(FIXTURES.glob("*.sx"))
TASK_HEADER = "; task: "


def task_path(source: str) -> Path:
    first_line = source.split("\n", 1)[0]
    assert first_line.startswith(TASK_HEADER), f"a checker fixture starts with {TASK_HEADER!r}"
    return TASKS / first_line.removeprefix(TASK_HEADER) / "task.json"


@pytest.mark.xfail(raises=ModuleNotFoundError, strict=True, reason="the checker arrives in Phase 1")
@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_checker_fixture(program: Path) -> None:
    source = program.read_text()
    task = json.loads(task_path(source).read_text())
    result = read(source)
    assert result.program is not None, render(source, result.diagnostics)
    checker = importlib.import_module("sx.checker")
    diagnostics = checker.check(result.program, task)
    assert render(source, diagnostics) == program.with_suffix(".expected").read_text()
