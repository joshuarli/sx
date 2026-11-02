from __future__ import annotations

from collections.abc import Mapping, Sequence
from dataclasses import dataclass, field
from typing import Any

from sx import ir
from sx.checker import DEFAULT_BUDGETS, Budgets
from sx.diagnostics import render
from sx.machine import (
    CallRequest,
    Completed,
    DelayRequest,
    Failed,
    Machine,
    Outcome,
    Output,
    Refusal,
    Response,
    Suspended,
)
from sx.program import compile_source
from sx.registry import Effect, Registry
from sx.types import Type
from sx.values import Value


@dataclass(slots=True)
class FixtureEntry:
    capability: str
    input: object
    results: Sequence[Mapping[str, Any]]
    served: int = 0


@dataclass(slots=True)
class FixtureHost:
    registry: Registry
    entries: list[FixtureEntry] = field(default_factory=list)
    delays: list[int] = field(default_factory=list)
    calls: list[dict[str, object]] = field(default_factory=list)

    @classmethod
    def from_responses(cls, registry: Registry, responses: Sequence[Mapping[str, Any]]) -> FixtureHost:
        entries = [FixtureEntry(r["capability"], r["input"], r["results"]) for r in responses]
        return cls(registry, entries)

    def answer(self, request: CallRequest | DelayRequest) -> Response:
        if isinstance(request, DelayRequest):
            self.delays.append(request.milliseconds)
            return Output(None)
        self.calls.append({"capability": request.capability, "input": request.input})
        entry = next((e for e in self.entries if e.capability == request.capability and e.input == request.input), None)
        if entry is None:
            raise UnexpectedCallError(request.capability, request.input)
        capability = self.registry.capability(request.capability)
        assert capability is not None
        if capability.effect is Effect.WRITE and entry.served >= len(entry.results):
            raise UnexpectedCallError(request.capability, request.input, repeated=True)
        result = entry.results[min(entry.served, len(entry.results) - 1)]
        entry.served += 1
        if "error" in result:
            return Refusal(result["error"]["kind"], result["error"]["message"])
        return Output(result["output"])

    def unperformed_writes(self) -> list[FixtureEntry]:
        writes = []
        for entry in self.entries:
            capability = self.registry.capability(entry.capability)
            if capability is not None and capability.effect is Effect.WRITE and entry.served != len(entry.results):
                writes.append(entry)
        return writes


class UnexpectedCallError(Exception):
    def __init__(self, capability: str, payload: object, *, repeated: bool = False) -> None:
        what = "was called more often than the fixture allows" if repeated else "has no fixture for this input"
        super().__init__(f"{capability} {what}: {payload!r}")
        self.capability = capability
        self.payload = payload


@dataclass(frozen=True, slots=True)
class Run:
    outcome: Outcome
    machine: Machine


def run(
    program: ir.Program,
    host: FixtureHost,
    inputs: Mapping[str, Value] | None = None,
    budgets: Budgets = DEFAULT_BUDGETS,
) -> Run:
    machine = Machine(program, inputs, budgets)
    outcome = machine.start()
    while isinstance(outcome, Suspended):
        outcome = machine.resume([(request.id, host.answer(request)) for request in outcome.requests])
    assert isinstance(outcome, Completed | Failed)
    return Run(outcome, machine)


class EvaluationError(Exception):
    pass


def evaluate(
    registry: Registry, source: str, inputs: Mapping[str, tuple[Type, Value]] | None = None
) -> Value:
    given = inputs or {}
    checked = compile_source(
        source, registry, allowed=frozenset(), inputs={name: t for name, (t, _) in given.items()}
    )
    if checked.program is None:
        raise EvaluationError(render(source, checked.diagnostics))
    outcome = run(checked.program, FixtureHost(registry), {name: v for name, (_, v) in given.items()}).outcome
    if not isinstance(outcome, Completed):
        assert isinstance(outcome, Failed)
        raise EvaluationError(f"{outcome.failure.kind.value}: {outcome.failure.message}")
    return outcome.value
