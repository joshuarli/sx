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
from sx.checker import DEFAULT_BUDGETS, Budgets, CheckResult
from sx.cli import EXIT_OK
from sx.diagnostics import render
from sx.export import export_program
from sx.host import FixtureHost
from sx.machine import (
    Closure,
    Completed,
    Failed,
    Fiber,
    Machine,
    Outcome,
    Output,
    Refusal,
    Response,
    Suspended,
)
from sx.program import compile_source
from sx.registry import Registry, load_declarations
from sx.syntax import Span
from sx.values import Value

from .generate import Gen
from .mutate import mutate
from .participants import Participant, participants
from .responses import Plan, ScriptHost
from .rust import DECLARATION_FILES, ROOT, KernelGoneError, Rust

BINARY = ROOT / "lean" / ".lake" / "build" / "bin" / "sx-ref"
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
    for path in DECLARATION_FILES:
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
    features: frozenset[str]


def make_case(seed: str, index: int, known: Registry, mutate_rate: float) -> Case:
    rng = random.Random(f"{seed}:{index}")
    budgets: dict[str, int] = {}
    if rng.random() < 0.3:
        names = rng.sample(sorted(BUDGET_RANGES), rng.choice([1, 1, 2]))
        if rng.random() < 0.4 and "steps" not in names:
            names.append("steps")
        for name in names:
            low, high = BUDGET_RANGES[name]
            budgets[name] = rng.randint(low, high)
    budgets.setdefault("steps", FUZZ_STEPS)
    generator = Gen(rng, known, budgets.get("concurrency", DEFAULT_BUDGETS.concurrency))
    source = generator.program()
    capabilities = sorted(known.capabilities)
    allow = tuple(capabilities)
    if rng.random() < 0.05:
        allow = tuple(rng.sample(capabilities, rng.randint(0, len(capabilities))))
    mutated = rng.random() < mutate_rate
    if mutated:
        source = mutate(source, rng)
    return Case(seed, index, source, allow, budgets, rng.random() < 0.35, mutated, frozenset(generator.features))


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
    participants: frozenset[Participant] = frozenset()


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


def anomalies(answers: list[tuple[str, dict[str, Any]]], rng: random.Random) -> list[tuple[str, dict[str, Any]]]:
    roll = rng.randrange(4)
    if roll == 0 and answers:
        return answers[:-1]
    if roll == 1 and answers:
        return [*answers, answers[0]]
    if roll == 2:
        return [*answers, ("r.9:9", {"output": None})]
    if answers:
        return [*answers[:-1], (answers[-1][0], {"output": "zz"})]
    return answers


def python_response(entry: Mapping[str, Any]) -> Response:
    if "output" in entry:
        return Output(entry["output"])
    return Refusal(entry["error"]["kind"], entry["error"]["message"])


def clip(text: str, limit: int = 300) -> str:
    return text if len(text) <= limit else text[:limit]


def finding_result(result: Result, kind: str, detail: str) -> Result:
    result.status, result.finding = "finding", Finding(kind, detail)
    return result


def refuses_what_rust_accepts(source: str, checked: CheckResult) -> bool:
    if checked.program is not None:
        return False
    codes = [d.code for d in checked.diagnostics]
    always_present = any("is always present, so get takes no default for it" in d.message for d in checked.diagnostics)
    capitalized_name = any(
        d.code == "bad-pattern" and d.message[:1].isupper() and " is not a pattern. A pattern is a name" in d.message
        for d in checked.diagnostics
    )
    return (
        codes == ["multiple-expressions"]
        or always_present
        or capitalized_name
        or ("(loop" in source and "unresolved-type" in codes)
    )


def check_against_python(
    case: Case, known: Registry, budgets: Budgets, rejected: dict[str, Any] | None
) -> tuple[CheckResult | None, Finding | None]:
    try:
        checked = compile_source(case.source, known, allowed=frozenset(case.allow), budgets=budgets)
    except Exception:
        return None, Finding("python-crash-check", traceback.format_exc())
    if refuses_what_rust_accepts(case.source, checked):
        return checked, None
    if rejected is None:
        if checked.program is None:
            codes = [d.code for d in checked.diagnostics]
            return checked, Finding("python-rejects", f"python {codes}\nrust accepted")
        return checked, None
    ours = [d["code"] for d in rejected["diagnostics"]]
    if checked.program is not None:
        return checked, Finding("python-accepts", f"python accepted\nrust {ours}")
    theirs = [d.code for d in checked.diagnostics]
    if theirs != ours:
        return checked, Finding("python-codes", f"python {theirs}\nrust {ours}")
    if render(case.source, checked.diagnostics) != rejected["rendered"]:
        theirs_text = render(case.source, checked.diagnostics)
        return checked, Finding("python-rendering", f"python {theirs_text}\nrust {rejected['rendered']}")
    return checked, None


def replay_in_python(
    program: ir.Program,
    budgets: Budgets,
    known: Registry,
    steps: list[dict[str, Any]] | None,
    responses: list[dict[str, Any]],
) -> tuple[CountingMachine, Outcome]:
    machine = CountingMachine(program, budgets)
    outcome = machine.start()
    if steps is None:
        host = FixtureHost.from_responses(known, responses)
        while isinstance(outcome, Suspended):
            outcome = machine.resume([(request.id, host.answer(request)) for request in outcome.requests])
        return machine, outcome
    for number, step in enumerate(steps):
        if not isinstance(outcome, Suspended):
            raise ReplayMismatchError(f"python finished before step {number}")
        asked = [request.id for request in outcome.requests]
        if asked != step["requests"]:
            raise ReplayMismatchError(f"step {number}: python asked for {asked}, rust for {step['requests']}")
        outcome = machine.resume([(request_id, python_response(entry)) for request_id, entry in step["respond"]])
    return machine, outcome


class ReplayMismatchError(Exception):
    pass


def outcome_of(reply: Mapping[str, Any]) -> tuple[dict[str, object], str]:
    if reply["type"] == "completed":
        return {"completed": reply["value"]}, "completed"
    kind = reply["failure"]["kind"]
    return {"failed": kind}, str(kind)


def exchange(
    rust: Rust, answers: list[tuple[str, dict[str, Any]]], sent: list[tuple[str, dict[str, Any]]]
) -> dict[str, Any]:
    responses = [{"id": request_id, **entry} for request_id, entry in sent]
    if len(sent) < len(answers):
        return rust.answer_some_then_cancel(responses)
    return rust.respond(responses)


def run_case(case: Case, known: Registry, rust: Rust) -> Result:
    try:
        result = judge(case, known, rust)
    except KernelGoneError as gone:
        result = Result("finding", finding=Finding("rust-crash", str(gone)))
    finally:
        rust.settle()
    result.participants = participants(case.features)
    return result


def judge(case: Case, known: Registry, rust: Rust) -> Result:
    who = participants(case.features)
    budgets = Budgets(**case.budgets)
    reply = rust.start(case.source, case.allow, case.budgets)
    if reply["type"] == "error":
        return Result("finding", finding=Finding("rust-error", f"{reply['code']}: {reply['message']}"))
    rejected = reply if reply["type"] == "rejected" else None
    checked: CheckResult | None = None
    if Participant.PYTHON in who:
        checked, disagreement = check_against_python(case, known, budgets, rejected)
        if disagreement is not None:
            return Result("finding", finding=disagreement)
    if rejected is not None:
        return Result("rejected", codes=[d["code"] for d in rejected["diagnostics"]])
    result = Result("agreed")
    program = checked.program if checked is not None else None
    if program is not None:
        result.nodes_static, result.ops_static = classes(program.body), operators(program.body)
    path = rust.work
    source_path, ir_path, fixture_path = path / "program.sx", path / "program.ir.json", path / "fixture.json"
    source_path.write_text(case.source)
    emitted = rust.emit_ir(source_path, case.allow)
    if emitted.returncode != EXIT_OK:
        return finding_result(result, "rust-check", f"exit {emitted.returncode}\n{emitted.stdout}{emitted.stderr}")
    result.ir = emitted.stdout
    ir_path.write_text(emitted.stdout)
    if program is not None:
        try:
            exported = json.dumps(export_program(program, {}))
        except Exception:
            return finding_result(result, "python-crash-export", traceback.format_exc())
        if json.dumps(json.loads(emitted.stdout)) != exported:
            return finding_result(result, "python-ir", f"python {clip(exported)}\nrust {clip(emitted.stdout)}")
    checked_by_lean = lean(["check", ir_path])
    if checked_by_lean.returncode != 0:
        return finding_result(result, "lean-rejects", checked_by_lean.stderr.strip())

    host = ScriptHost(known, f"{case.seed}:{case.index}", Plan())
    rng = random.Random(f"{case.seed}:{case.index}:anomaly")
    steps: list[dict[str, Any]] = []
    while reply["type"] == "requests":
        answers = [(request["id"], host.answer(request)) for request in reply["requests"]]
        sent = anomalies(answers, rng) if case.steps_mode and rng.random() < 0.04 else answers
        asked = [request["id"] for request in reply["requests"]]
        steps.append({"requests": asked, "respond": [list(item) for item in sent]})
        result.requests += len(answers)
        reply = exchange(rust, answers, sent)
    if reply["type"] not in ("completed", "failed"):
        return finding_result(result, "rust-reply", json.dumps(reply)[:400])
    expected, result.outcome = outcome_of(reply)
    trace: list[Any] = reply["trace"]
    if reply["type"] == "failed" and result.outcome == "internal":
        return finding_result(result, "rust-internal", str(reply["failure"]["message"]))
    fixture: dict[str, Any] = {"budgets": dict(case.budgets)}
    if case.steps_mode:
        fixture["steps"] = steps
    else:
        fixture["responses"] = host.fixture_responses()
    result.fixture = json.dumps(fixture)
    fixture_path.write_text(result.fixture)
    if program is not None:
        problem = python_problem(program, budgets, known, fixture, expected, trace, result)
        if problem is not None:
            return finding_result(result, *problem)
    try:
        ran = lean([ir_path, fixture_path])
    except subprocess.TimeoutExpired:
        return finding_result(result, "lean-timeout", "")
    if ran.returncode != 0:
        return finding_result(result, "lean-error", ran.stderr.strip())
    produced = json.loads(ran.stdout)
    if not same(produced["outcome"], expected):
        return finding_result(
            result, "outcome", f"rust {json.dumps(expected)[:300]}\nlean {json.dumps(produced['outcome'])[:300]}"
        )
    if not same(produced["trace"], trace):
        index = next(
            (i for i, (a, b) in enumerate(zip(produced["trace"], trace, strict=False)) if not same(a, b)),
            min(len(produced["trace"]), len(trace)),
        )
        detail = f"event {index}: rust {trace[index:index + 1]}\nlean {produced['trace'][index:index + 1]}"
        return finding_result(result, "trace", detail)
    return result


def python_problem(
    program: ir.Program,
    budgets: Budgets,
    known: Registry,
    fixture: dict[str, Any],
    expected: dict[str, object],
    trace: list[Any],
    result: Result,
) -> tuple[str, str] | None:
    try:
        machine, outcome = replay_in_python(program, budgets, known, fixture.get("steps"), fixture.get("responses", []))
    except ReplayMismatchError as mismatch:
        return "python-replay", str(mismatch)
    except Exception:
        return "python-crash-run", traceback.format_exc()
    result.nodes_run, result.ops_run = machine.nodes_run, machine.ops_run
    if isinstance(outcome, Completed):
        theirs: dict[str, object] = {"completed": outcome.json}
    elif isinstance(outcome, Failed):
        theirs = {"failed": outcome.failure.kind.value}
    else:
        assert isinstance(outcome, Suspended)
        return "python-outcome", f"python suspended\nrust {json.dumps(expected)[:300]}"
    if not same(theirs, expected):
        return "python-outcome", f"python {json.dumps(theirs)[:300]}\nrust {json.dumps(expected)[:300]}"
    if not same(machine.trace, trace):
        index = next(
            (i for i, (a, b) in enumerate(zip(machine.trace, trace, strict=False)) if not same(a, b)),
            min(len(machine.trace), len(trace)),
        )
        return "python-trace", f"event {index}: python {machine.trace[index:index + 1]}\nrust {trace[index:index + 1]}"
    return None


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
    participants: Counter[str] = field(default_factory=Counter)
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
        self.participants.update(p.value for p in result.participants)
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
        self.participants.update(other.participants)
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
                "steps_mode": case.steps_mode, "mutated": case.mutated, "features": sorted(case.features),
            },
            indent=1,
        )
    )
    (target / "program.ir.json").write_text(result.ir)
    (target / "fixture.json").write_text(result.fixture)
    if result.finding is not None:
        (target / "finding.txt").write_text(f"{result.finding.kind}\n{result.finding.detail}\n")
    return target


def scratch() -> Path:
    return ROOT / ".work" / "tmp" / "fuzz" / f"run-{os.getpid()}"


def run_chunk(seed: str, indexes: Sequence[int], mutate_rate: float, keep: Path) -> Totals:
    known = registry()
    totals = Totals()
    rust = Rust(scratch())
    try:
        for index in indexes:
            case = make_case(seed, index, known, mutate_rate)
            result = run_case(case, known, rust)
            totals.add(case, result)
            if result.finding is not None:
                save_finding(case, result, keep)
    finally:
        rust.reset()
    return totals
