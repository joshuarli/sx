import importlib
from pathlib import Path

import pytest

from sx.diagnostics import render
from sx.reader import read

FIXTURES = Path(__file__).parent.parent / "fixtures" / "check"
PROGRAMS = sorted(FIXTURES.glob("*.sx"))


@pytest.mark.xfail(raises=ModuleNotFoundError, strict=True, reason="the checker arrives in Phase 1")
@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_checker_fixture(program: Path) -> None:
    checker = importlib.import_module("sx.checker")
    source = program.read_text()
    result = read(source)
    assert result.program is not None, render(source, result.diagnostics)
    diagnostics = checker.check(result.program)
    assert render(source, diagnostics) == program.with_suffix(".expected").read_text()
