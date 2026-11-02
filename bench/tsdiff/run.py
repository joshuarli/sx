from __future__ import annotations

from collections import Counter
from collections.abc import Iterable
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass, field
from pathlib import Path

from .case import META, CaseFiles
from .compare import Outcome, SxProtocolError, Verdict, compare, run_js, run_sx

PARALLEL_CONSTRUCTS = ("Promise.all", "parMap")


@dataclass(frozen=True, slots=True)
class Result:
    name: str
    verdict: Verdict | None
    js: Outcome | None
    sx: Outcome | None
    mutant: str | None = None
    expected_code: str | None = None
    caught: bool | None = None
    note: str = ""


@dataclass(slots=True)
class Report:
    results: list[Result] = field(default_factory=list)

    def disagreements(self) -> list[Result]:
        return [r for r in self.results if r.verdict is not None and r.verdict.kind == "disagree"]

    def accepted_mutants(self) -> list[Result]:
        return [r for r in self.results if r.caught is False]

    def clean(self) -> bool:
        return not self.disagreements() and not self.accepted_mutants()


def case_directories(roots: Iterable[Path]) -> list[Path]:
    found: list[Path] = []
    for root in roots:
        found += [root] if (root / META).is_file() else sorted(p.parent for p in root.glob(f"*/{META}"))
    return found


def judge_mutant(files: CaseFiles, sx: Outcome) -> Result:
    expected = str(files.meta["expect_code"])
    caught = sx.status == "rejected"
    note = ""
    if caught and expected not in sx.codes:
        note = f"rejected with {list(sx.codes)}, expected {expected}"
    return Result(files.name, None, None, sx, str(files.meta["mutant"]), expected, caught, note)


def run_case(directory: Path, *, shim_only: bool, binary: Path | None) -> Result:
    files = CaseFiles.read(directory)
    if "mutant" in files.meta:
        if shim_only:
            return Result(files.name, None, None, None, str(files.meta["mutant"]), str(files.meta["expect_code"]))
        return judge_mutant(files, run_sx(files, binary))
    js = run_js(files)
    if shim_only:
        return Result(files.name, None, js, None)
    sx = run_sx(files, binary)
    parallel = any(construct in files.program.read_text() for construct in PARALLEL_CONSTRUCTS)
    return Result(files.name, compare(js, sx, parallel=parallel, expect_trap=files.expect_trap), js, sx)


def run_all(directories: list[Path], *, shim_only: bool, binary: Path | None, jobs: int) -> Report:
    report = Report()
    with ThreadPoolExecutor(max_workers=jobs) as pool:
        report.results = list(pool.map(lambda d: run_case(d, shim_only=shim_only, binary=binary), directories))
    return report


def summarize(report: Report) -> list[str]:
    lines: list[str] = []
    verdicts = Counter(r.verdict.kind for r in report.results if r.verdict is not None)
    lines.append(f"cases {len(report.results)}  verdicts {dict(verdicts.most_common())}")
    codes = Counter(code for r in report.results if r.verdict is not None and r.verdict.kind == "rejected" for code in r.verdict.reasons)
    if codes:
        lines.append(f"rejected by sx, by diagnostic code: {dict(codes.most_common())}")
    shapes = Counter(
        (r.js.failure.cls if r.js.failure else "completed") for r in report.results if r.js is not None
    )
    if shapes:
        lines.append(f"js outcomes {dict(shapes.most_common())}")
    mutants = [r for r in report.results if r.mutant is not None]
    if mutants and any(r.caught is not None for r in mutants):
        caught = sum(1 for r in mutants if r.caught)
        wrong = [r for r in mutants if r.caught and r.note]
        lines.append(f"mutants {len(mutants)}  rejected {caught}  accepted {len(report.accepted_mutants())}  other code {len(wrong)}")
    for r in report.disagreements():
        lines.append(f"DISAGREE {r.name}: " + "; ".join(r.verdict.reasons if r.verdict else ()))
    for r in report.accepted_mutants():
        lines.append(f"MUTANT ACCEPTED {r.name}: {r.mutant} should be {r.expected_code}")
    for r in report.results:
        if r.caught and r.note:
            lines.append(f"mutant {r.name} ({r.mutant}): {r.note}")
    return lines


__all__ = ["Report", "Result", "SxProtocolError", "case_directories", "run_all", "summarize"]
