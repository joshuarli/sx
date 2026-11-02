from __future__ import annotations

import json
import os
import random
import subprocess
import traceback
from collections import Counter
from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field, fields, is_dataclass
from pathlib import Path
from typing import Any, cast, override

from sx import ir
from sx.checker import DEFAULT_BUDGETS, Budgets
from sx.export import export_program
from sx.machine import (
    Closure,
    Completed,
    Failed,
    Fiber,
    Machine,
    Output,
    Response,
    Suspended,
)
from sx.program import compile_source
from sx.registry import Registry, load_declarations
from sx.syntax import Span
from sx.values import Value

from .generate import Gen
from .mutate import mutate
from .responses import Plan, ScriptHost

ROOT = Path(__file__).resolve().parent.parent.parent
BINARY = ROOT / "lean" / ".lake" / "build" / "bin" / "sx-ref"
DECLARATIONS = (ROOT / "fixtures" / "run" / "declarations.json", Path(__file__).resolve().parent / "declarations.json")
BUDGET_RANGES: dict[str, tuple[int, int]] = {
    "steps": (20, 600),
    "calls": (1, 6),
    "delay_ms": (500, 90_000),
    "value_size": (4, 120),
    "applications": (3, 80),
    "concurrency": (1, 4),
}
LEAN_TIMEOUT = 60
FUZZ_STEPS = 100_000


def registry() -> Registry:
    loaded = Registry()
    for path in DECLARATIONS:
        load_declarations(loaded, json.loads(path.read_text()))
    return loaded


@dataclass(frozen=True, slots=True)
class Case:
    seed: str
    index: int
    source: str
    allow: tuple[str, ...]
    budgets: Mapping[str, int]
    steps_mode: bool
    mutated: bool


def make_case(seed: str, index: int, known: Registry, mutate_rate: float) -> Case:
    rng = random.Random(f"{seed}:{index}")
    budgets: dict[str, int] = {}
    if rng.random() < 0.25:
        for name in rng.sample(sorted(BUDGET_RANGES), rng.choice([1, 1, 2])):
            low, high = BUDGET_RANGES[name]
            budgets[name] = rng.randint(low, high)
    budgets.setdefault("steps", FUZZ_STEPS)
    source = Gen(rng, known, budgets.get("concurrency", DEFAULT_BUDGETS.concurrency)).program()
    capabilities = sorted(known.capabilities)
    allow = tuple(capabilities)
    if rng.random() < 0.05:
        allow = tuple(rng.sample(capabilities, rng.randint(0, len(capabilities))))
    mutated = rng.random() < mutate_rate
    if mutated:
        source = mutate(source, rng)
    return Case(seed, index, source, allow, budgets, rng.random() < 0.35, mutated)


@dataclass(slots=True)
class Finding:
    kind: str
    detail: str

    @property
    def signature(self) -> str:
        lines = self.detail.strip().splitlines()
        if not lines:
            return self.kind
        line = lines[-1] if self.kind.startswith("python-crash") else lines[0]
        return f"{self.kind}: {line[:120]}"


@dataclass(slots=True)
class Result:
    status: str
    codes: list[str] = field(default_factory=list)
    finding: Finding | None = None
    nodes_static: set[str] = field(default_factory=set)
    nodes_run: set[str] = field(default_factory=set)
    ops_static: set[str] = field(default_factory=set)
    ops_run: set[str] = field(default_factory=set)
    outcome: str = ""
    requests: int = 0
    ir: str = ""
    fixture: str = ""


class CountingMachine(Machine):
    def __init__(self, program: ir.Program, budgets: Budgets) -> None:
        super().__init__(program, None, budgets)
        self.nodes_run: set[str] = set()
        self.ops_run: set[str] = set()

    @override
    def evaluate(self, fiber: Fiber, node: ir.Node, env: Any) -> None:
        self.nodes_run.add(type(node).__name__)
        match node:
            case ir.Coerce(conversion=conversion):
                self.nodes_run.update(classes(conversion))
            case ir.Match(clauses=clauses):
                for pattern, _ in clauses:
                    self.nodes_run.update(classes(pattern))
            case ir.Let(binder=binder):
                self.nodes_run.update(classes(binder))
        super().evaluate(fiber, node, env)

    @override
    def prelude(self, fiber: Fiber, op: str, values: list[Value | Closure], span: Span) -> None:
        self.ops_run.add(op)
        super().prelude(fiber, op, values, span)


def classes(node: object) -> set[str]:
    found: set[str] = set()
    if isinstance(node, tuple | frozenset | list):
        for item in node:
            found |= classes(item)
    elif is_dataclass(node) and type(node).__module__ == "sx.ir":
        found.add(type(node).__name__)
        for f in fields(node):
            found |= classes(getattr(node, f.name))
    return found


def operators(node: object) -> set[str]:
    found: set[str] = set()
    if isinstance(node, tuple | frozenset | list):
        for item in node:
            found |= operators(item)
    elif is_dataclass(node) and type(node).__module__ == "sx.ir":
        if isinstance(node, ir.Prelude):
            found.add(node.op)
        for f in fields(node):
            found |= operators(getattr(node, f.name))
    return found


def response_json(response: Response) -> dict[str, Any]:
    if isinstance(response, Output):
        return {"output": response.value}
    return {"error": {"kind": response.kind, "message": response.message}}


def same(a: object, b: object) -> bool:
    if isinstance(a, bool) or isinstance(b, bool):
        return isinstance(a, bool) and isinstance(b, bool) and a == b
    if isinstance(a, int | float) and isinstance(b, int | float):
        if isinstance(a, int) and isinstance(b, int):
            return a == b
        return float(a) == float(b)
    if isinstance(a, dict) and isinstance(b, dict):
        left, right = cast("dict[str, object]", a), cast("dict[str, object]", b)
        return left.keys() == right.keys() and all(same(left[k], right[k]) for k in left)
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same(x, y) for x, y in zip(a, b, strict=True))
    return a == b


def lean(args: Sequence[str | Path]) -> subprocess.CompletedProcess[str]:
    return subprocess.run([BINARY, *args], capture_output=True, text=True, check=False, timeout=LEAN_TIMEOUT)


def anomalies(answers: list[tuple[str, Response]], rng: random.Random) -> list[tuple[str, Response]]:
    roll = rng.randrange(4)
    if roll == 0 and answers:
        return answers[:-1]
    if roll == 1 and answers:
        return [*answers, answers[0]]
    if roll == 2:
        return [*answers, ("r.9:9", Output(None))]
    if answers:
        return [*answers[:-1], (answers[-1][0], Output("zz"))]
    return answers


def run_case(case: Case, known: Registry, work: Path | None = None) -> Result:
    budgets = Budgets(**case.budgets)
    try:
        checked = compile_source(case.source, known, allowed=frozenset(case.allow), budgets=budgets)
    except Exception:
        return Result("finding", finding=Finding("python-crash-check", traceback.format_exc()))
    if checked.program is None:
        return Result("rejected", codes=[d.code for d in checked.diagnostics])
    program = checked.program
    result = Result("agreed", nodes_static=classes(program.body), ops_static=operators(program.body))
    try:
        exported = json.dumps(export_program(program, {}))
    except Exception:
        result.status, result.finding = "finding", Finding("python-crash-export", traceback.format_exc())
        return result
    result.ir = exported
    path = work or ROOT / ".work" / "tmp" / "fuzz" / f"run-{os.getpid()}"
    path.mkdir(parents=True, exist_ok=True)
    ir_path, fixture_path = path / "program.ir.json", path / "fixture.json"
    ir_path.write_text(exported)
    checked_by_lean = lean(["check", ir_path])
    if checked_by_lean.returncode != 0:
        result.status = "finding"
        result.finding = Finding("lean-rejects", checked_by_lean.stderr.strip())
        return result

    host = ScriptHost(known, f"{case.seed}:{case.index}", Plan())
    rng = random.Random(f"{case.seed}:{case.index}:anomaly")
    steps: list[dict[str, Any]] = []
    try:
        machine = CountingMachine(program, budgets)
        outcome = machine.start()
        while isinstance(outcome, Suspended):
            answers = [(request.id, host.answer(request)) for request in outcome.requests]
            sent = answers
            if case.steps_mode and rng.random() < 0.04:
                sent = anomalies(answers, rng)
            steps.append(
                {
                    "requests": [request.id for request in outcome.requests],
                    "respond": [[request_id, response_json(response)] for request_id, response in sent],
                }
            )
            result.requests += len(answers)
            outcome = machine.resume(sent)
    except Exception:
        result.status, result.finding = "finding", Finding("python-crash-run", traceback.format_exc())
        return result
    result.nodes_run, result.ops_run = machine.nodes_run, machine.ops_run
    if isinstance(outcome, Completed):
        expected: dict[str, object] = {"completed": outcome.json}
        result.outcome = "completed"
    else:
        assert isinstance(outcome, Failed)
        expected = {"failed": outcome.failure.kind.value}
        result.outcome = outcome.failure.kind.value
        if outcome.failure.kind.value == "internal":
            result.status, result.finding = "finding", Finding("python-internal", outcome.failure.message)
    fixture: dict[str, Any] = {"budgets": dict(case.budgets)}
    if case.steps_mode:
        fixture["steps"] = steps
    else:
        fixture["responses"] = host.fixture_responses()
    result.fixture = json.dumps(fixture)
    fixture_path.write_text(result.fixture)
    try:
        ran = lean([ir_path, fixture_path])
    except subprocess.TimeoutExpired:
        result.status, result.finding = "finding", Finding("lean-timeout", "")
        return result
    if ran.returncode != 0:
        result.status = "finding"
        result.finding = Finding("lean-error", ran.stderr.strip())
        return result
    produced = json.loads(ran.stdout)
    if not same(produced["outcome"], expected):
        detail = f"python {json.dumps(expected)[:300]}\nlean {json.dumps(produced['outcome'])[:300]}"
        result.status, result.finding = "finding", Finding("outcome", detail)
    elif not same(produced["trace"], machine.trace):
        index = next(
            (i for i, (a, b) in enumerate(zip(produced["trace"], machine.trace, strict=False)) if not same(a, b)),
            min(len(produced["trace"]), len(machine.trace)),
        )
        detail = f"event {index}: python {machine.trace[index:index + 1]}\nlean {produced['trace'][index:index + 1]}"
        result.status, result.finding = "finding", Finding("trace", detail)
    return result


@dataclass(slots=True)
class Totals:
    cases: int = 0
    mutated: int = 0
    status: Counter[str] = field(default_factory=Counter)
    rejected_codes: Counter[str] = field(default_factory=Counter)
    outcomes: Counter[str] = field(default_factory=Counter)
    nodes_static: Counter[str] = field(default_factory=Counter)
    nodes_run: Counter[str] = field(default_factory=Counter)
    ops_static: Counter[str] = field(default_factory=Counter)
    ops_run: Counter[str] = field(default_factory=Counter)
    requests: int = 0
    findings: list[tuple[int, Finding]] = field(default_factory=list)

    def add(self, case: Case, result: Result) -> None:
        self.cases += 1
        self.mutated += case.mutated
        self.status[result.status] += 1
        self.rejected_codes.update(result.codes[:1])
        if result.outcome:
            self.outcomes[result.outcome] += 1
        self.nodes_static.update(result.nodes_static)
        self.nodes_run.update(result.nodes_run)
        self.ops_static.update(result.ops_static)
        self.ops_run.update(result.ops_run)
        self.requests += result.requests
        if result.finding is not None:
            self.findings.append((case.index, result.finding))

    def merge(self, other: Totals) -> None:
        self.cases += other.cases
        self.mutated += other.mutated
        self.status.update(other.status)
        self.rejected_codes.update(other.rejected_codes)
        self.outcomes.update(other.outcomes)
        self.nodes_static.update(other.nodes_static)
        self.nodes_run.update(other.nodes_run)
        self.ops_static.update(other.ops_static)
        self.ops_run.update(other.ops_run)
        self.requests += other.requests
        self.findings.extend(other.findings)


def ir_classes() -> list[str]:
    return sorted(name for name, value in vars(ir).items() if is_dataclass(value) and value.__module__ == "sx.ir")


def save_finding(case: Case, result: Result, directory: Path) -> Path:
    target = directory / f"{case.seed.replace('/', '_')}-{case.index}"
    target.mkdir(parents=True, exist_ok=True)
    (target / "program.sx").write_text(case.source)
    (target / "case.json").write_text(
        json.dumps(
            {
                "seed": case.seed, "index": case.index, "allow": list(case.allow), "budgets": dict(case.budgets),
                "steps_mode": case.steps_mode, "mutated": case.mutated,
            },
            indent=1,
        )
    )
    (target / "program.ir.json").write_text(result.ir)
    (target / "fixture.json").write_text(result.fixture)
    if result.finding is not None:
        (target / "finding.txt").write_text(f"{result.finding.kind}\n{result.finding.detail}\n")
    return target


def run_chunk(seed: str, indexes: Sequence[int], mutate_rate: float, keep: Path) -> Totals:
    known = registry()
    totals = Totals()
    for index in indexes:
        case = make_case(seed, index, known, mutate_rate)
        result = run_case(case, known)
        totals.add(case, result)
        if result.finding is not None:
            save_finding(case, result, keep)
    return totals
