import json
import resource
import sys
from dataclasses import dataclass
from pathlib import Path
from typing import Any

from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_repomod.kernel import Kernel
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx import ir
from sx.checker import Budgets, check
from sx.host import FixtureHost, run
from sx.machine import Completed, Machine, Output, Suspended
from sx.program import compile_source
from sx.reader import read
from sx.registry import Registry, load_declarations
from sx.types import Type
from sx.values import Value, decode

CORES = {core.id: core for core in (TEXT, SEMVER, GIT, GITHUB)}
CONSTANT = {"id": "a", "size": 1}


@dataclass
class Case:
    source: str
    declarations: dict[str, Any]
    registry: Registry
    declared: dict[str, Type]
    allow: frozenset[str]
    budgets: Budgets
    inputs: dict[str, Value]
    responses: list[dict[str, Any]]
    raw_inputs: dict[str, Any]


def case_of(raw: dict[str, Any]) -> Case:
    declarations = raw["declarations"]
    registry = Registry()
    for core_id in declarations.get("cores", []):
        registry.add_core(CORES[core_id])
    load_declarations(registry, declarations)
    declared = {name: registry.parse(text) for name, text in declarations.get("inputs", {}).items()}
    return Case(
        source=raw["source"],
        declarations=declarations,
        registry=registry,
        declared=declared,
        allow=frozenset(raw["allow"]),
        budgets=Budgets(**raw["budgets"]),
        inputs={name: decode(value, declared[name], f"${name}") for name, value in raw["inputs"].items()},
        responses=raw["responses"],
        raw_inputs=raw["inputs"],
    )


def cases(bundle: dict[str, Any], group: str) -> list[Case]:
    if group in ("corpus", "status"):
        return [case_of(raw) for raw in bundle[group]]
    return [case_of(bundle["synthetic"][group])]


def compiled(case: Case) -> ir.Program:
    checked = compile_source(case.source, case.registry, allowed=case.allow, inputs=case.declared, budgets=case.budgets)
    assert checked.program is not None, case.source
    return checked.program


def answer_all(program: ir.Program, case: Case) -> int:
    machine = Machine(program, case.inputs, case.budgets)
    outcome = machine.start()
    answered = 0
    while isinstance(outcome, Suspended):
        answered += len(outcome.requests)
        outcome = machine.resume([(request.id, Output(CONSTANT)) for request in outcome.requests])
    assert isinstance(outcome, Completed)
    return answered


def through_protocol(kernel: Kernel, case: Case) -> None:
    reply = kernel.send(
        {
            "type": "start",
            "program": case.source,
            "declarations": case.declarations,
            "allow": sorted(case.allow),
            "inputs": case.raw_inputs,
        }
    )
    while reply["type"] == "requests":
        responses = [{"id": request["id"], "output": CONSTANT} for request in reply["requests"]]
        reply = kernel.send({"type": "respond", "responses": responses})
    assert reply["type"] == "completed", reply


def main(bundle_path: str, operation: str, count: str) -> None:
    iterations = int(count)
    bundle = json.loads(Path(bundle_path).read_text())
    stage, _, group = operation.partition("-")
    listed = cases(bundle, group)
    if stage == "parse":
        for _ in range(iterations):
            for case in listed:
                read(case.source)
    elif stage == "check":
        parsed = [read(case.source).program for case in listed]
        for _ in range(iterations):
            for case, node in zip(listed, parsed, strict=True):
                assert node is not None
                check(node, case.registry, allowed=case.allow, inputs=case.declared, budgets=case.budgets)
    elif stage == "run":
        programs = [compiled(case) for case in listed]
        if group in ("corpus", "status"):
            hosts = [FixtureHost.from_responses(case.registry, case.responses) for case in listed]
            for _ in range(iterations):
                for program, case, host in zip(programs, listed, hosts, strict=True):
                    for entry in host.entries:
                        entry.served = 0
                    host.delays.clear()
                    host.calls.clear()
                    run(program, host, case.inputs, case.budgets)
        else:
            for _ in range(iterations):
                for program, case in zip(programs, listed, strict=True):
                    answer_all(program, case)
    elif stage == "protocol":
        with Kernel() as kernel:
            for _ in range(iterations):
                for case in listed:
                    through_protocol(kernel, case)
        spent = resource.getrusage(resource.RUSAGE_CHILDREN)
        print(spent.ru_utime + spent.ru_stime)
    else:
        raise SystemExit(f"unknown stage {stage}")


if __name__ == "__main__":
    main(*sys.argv[1:])
