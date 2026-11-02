import json
import re
from pathlib import Path
from typing import cast

import pytest
from cores import task_registry as declared_registry

from sx.checker import check
from sx.diagnostics import render
from sx.reader import read
from sx.registry import Registry

ROOT = Path(__file__).parent.parent
FIXTURES = ROOT / "fixtures" / "check"
BENCH = ROOT / "bench" / "codemode"
PROGRAMS = sorted(FIXTURES.glob("*.sx"))
TASK_HEADER = "; task: "
ALLOW_HEADER = "; allow: "
LANGUAGE = ROOT / "docs" / "language.md"


def task_registry(task_id: str) -> tuple[Registry, frozenset[str]]:
    task = json.loads((BENCH / "tasks" / task_id / "task.json").read_text())
    capability_ids = frozenset(cast(str, capability["id"]) for capability in task["capabilities"])
    return declared_registry(task), capability_ids


@pytest.mark.parametrize("program", PROGRAMS, ids=[path.stem for path in PROGRAMS])
def test_checker_fixture(program: Path) -> None:
    source = program.read_text()
    first_line = source.split("\n", 1)[0]
    assert first_line.startswith(TASK_HEADER), f"a checker fixture starts with {TASK_HEADER!r}"
    registry, allowed = task_registry(first_line.removeprefix(TASK_HEADER))
    second_line = source.split("\n", 2)[1]
    if second_line.startswith(ALLOW_HEADER):
        allowed = frozenset(second_line.removeprefix(ALLOW_HEADER).split())
    result = read(source)
    assert result.program is not None, render(source, result.diagnostics)
    checked = check(result.program, registry, allowed=allowed)
    assert render(source, checked.diagnostics) == program.with_suffix(".expected").read_text()


def specified_codes() -> set[str]:
    text = LANGUAGE.read_text()
    table = text[text.index("The checker's codes are:") : text.index("A habit diagnostic names")]
    codes: set[str] = set()
    for row in table.splitlines():
        if not row.startswith("| ") or row.startswith(("| Group", "| ---")):
            continue
        group, names = row.split("|")[1:3]
        found = re.findall(r"`([a-z?-]+)`", names)
        if group.strip() == "Habits":
            codes.update(f"habit:{name}" for name in found if name != "habit")
        elif group.strip() == "Size":
            codes.add("too-large")
        else:
            codes.update(found)
    return codes


def test_every_specified_diagnostic_code_has_a_pinned_fixture() -> None:
    pinned = set()
    for expected in [*FIXTURES.glob("*.expected"), *(ROOT / "fixtures" / "read").glob("*.expected")]:
        pinned.update(re.findall(r"^error\[([a-z:?-]+)\]", expected.read_text(), flags=re.MULTILINE))
    assert sorted(specified_codes() - pinned) == []
