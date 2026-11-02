from __future__ import annotations

import time
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass

from sx.checker import DEFAULT_BUDGETS
from sx.diagnostics import render
from sx.machine import (
    CallRequest,
    Completed,
    DelayRequest,
    Failed,
    Machine,
    Output,
    Refusal,
    Request,
    Response,
    Suspended,
)
from sx.program import compile_source
from sx.registry import Registry
from sx.values import Value, decode
from sx_repomod.capabilities import Capabilities, CapabilityError
from sx_repomod.declarations import Program, new_registry

MAX_WORKERS = 8


class RejectedProgramError(Exception):
    pass


@dataclass(frozen=True, slots=True)
class Succeeded:
    result: object


@dataclass(frozen=True, slots=True)
class Errored:
    message: str


type Outcome = Succeeded | Errored


def answer(capabilities: Capabilities, request: Request, refusals: list[Refusal]) -> Response:
    if isinstance(request, DelayRequest):
        time.sleep(request.milliseconds / 1000)
        return Output(None)
    assert isinstance(request, CallRequest)
    try:
        return Output(capabilities.call(request.capability, request.input))
    except CapabilityError as failure:
        refusal = Refusal(failure.kind, failure.message)
        refusals.append(refusal)
        return refusal


def run_program(
    program: Program, inputs: Mapping[str, object], capabilities: Capabilities, registry: Registry | None = None
) -> Outcome:
    registry = registry or new_registry()
    types = program.input_types(registry)
    source = program.source
    checked = compile_source(source, registry, allowed=program.allow, inputs=types)
    if checked.program is None:
        raise RejectedProgramError(render(source, checked.diagnostics))
    values: dict[str, Value] = {name: decode(inputs[name], types[name], f"${name}") for name in types}
    machine = Machine(checked.program, values, DEFAULT_BUDGETS)
    refusals: list[Refusal] = []
    outcome = machine.start()
    with ThreadPoolExecutor(max_workers=MAX_WORKERS, thread_name_prefix="sx-repomod") as pool:
        while isinstance(outcome, Suspended):
            responses = list(pool.map(lambda r: answer(capabilities, r, refusals), outcome.requests))
            answered = zip(outcome.requests, responses, strict=True)
            outcome = machine.resume([(request.id, response) for request, response in answered])
    if isinstance(outcome, Completed):
        return Succeeded(outcome.json)
    assert isinstance(outcome, Failed)
    message = outcome.failure.message
    for refusal in refusals:
        if message == f"A tool call failed with {refusal.kind}: {refusal.message}":
            return Errored(refusal.message)
    return Errored(f"{outcome.failure.kind.value}: {message}")
