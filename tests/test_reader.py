from pathlib import Path

import pytest

from sx.diagnostics import render
from sx.reader import read
from sx.syntax import show

FIXTURES = Path(__file__).parent.parent / "fixtures" / "read"
PROGRAMS = sorted(FIXTURES.glob("*.sx"))


def outcome(source: str) -> str:
    result = read(source)
    if result.program is not None:
        return f"ok: {show(result.program)}\n"
    return render(source, result.diagnostics)


@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_reader_fixture(program: Path) -> None:
    expected = program.with_suffix(".expected").read_text()
    assert outcome(program.read_text()) == expected


@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_canonical_form_reads_back_to_itself(program: Path) -> None:
    result = read(program.read_text())
    if result.program is None:
        return
    canonical = show(result.program)
    again = read(canonical)
    assert again.program is not None
    assert show(again.program) == canonical


def test_reports_more_errors_than_the_limit_as_a_count() -> None:
    source = " ".join(["1."] * 23)
    text = render(source, read(f"[{source}]").diagnostics)
    assert text.count("error[bad-literal]") == 20
    assert text.endswith("3 more errors not shown.\n")


def test_error_after_an_unclosed_bracket_is_still_reported() -> None:
    result = read('(let [x 1.]\n  (f "a\\q"')
    codes = [diagnostic.code for diagnostic in result.diagnostics]
    assert codes == ["bad-literal", "bad-escape", "unclosed", "unclosed"]
