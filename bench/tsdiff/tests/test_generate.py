from __future__ import annotations

import random
import re
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from tsdiff.build import generated_case, mutant_case
from tsdiff.case import CaseFiles
from tsdiff.compare import run_js
from tsdiff.generate import KINDS, write_program
from tsdiff.mutants import MUTANTS, mutate

SPEC = Path(__file__).resolve().parents[3] / "docs" / "checkable-typescript.md"
SEEDS = range(120)


def test_a_seed_gives_the_same_program_and_the_same_responses() -> None:
    first, second = generated_case(7), generated_case(7)
    assert first.program == second.program
    assert first.responses == second.responses
    assert write_program(7) != write_program(8)


def test_mutation_is_deterministic_and_places_the_snippet_first() -> None:
    program = write_program(3)
    mutated, mutant = mutate(program, random.Random(1))
    assert mutated == mutate(program, random.Random(1))[0]
    assert mutated == mutant.snippet + "\n" + program
    assert mutant_case(3).meta == mutant_case(3).meta


def test_every_mutant_expects_a_code_the_spec_defines() -> None:
    defined = set(re.findall(r"^\| `([a-z]+(?:-[a-z]+)+)` \|", SPEC.read_text(), re.MULTILINE))
    assert {m.code for m in MUTANTS} <= defined
    assert len({m.name for m in MUTANTS}) == len(MUTANTS)


def test_generated_programs_run_on_the_shim_without_a_javascript_error_and_cover_every_call_error_kind(
    tmp_path: Path,
) -> None:
    def run(seed: int) -> tuple[str, set[str]]:
        case = generated_case(seed)
        files = CaseFiles.read(case.write(tmp_path / case.name))
        outcome = run_js(files)
        kinds: set[str] = {r["error"]["kind"] for entry in case.responses for r in entry["results"] if "error" in r}
        return (outcome.failure.cls if outcome.failure else "completed"), kinds

    with ThreadPoolExecutor(8) as pool:
        results = list(pool.map(run, SEEDS))
    classes = {cls for cls, _ in results}
    assert "js-error" not in classes
    assert "completed" in classes
    assert {"arithmetic", "out-of-range"} & classes
    assert set().union(*(kinds for _, kinds in results)) == set(KINDS)
