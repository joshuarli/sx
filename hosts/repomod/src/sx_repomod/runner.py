from __future__ import annotations

import time
from collections.abc import Mapping
from concurrent.futures import ThreadPoolExecutor
from dataclasses import dataclass
from typing import Any

from sx_repomod.capabilities import Capabilities, CapabilityError
from sx_repomod.declarations import Program
from sx_repomod.kernel import Kernel

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


@dataclass(frozen=True, slots=True)
class Refusal:
    kind: str
    message: str


def answer(capabilities: Capabilities, request: Mapping[str, Any], refusals: list[Refusal]) -> dict[str, Any]:
    if request["kind"] == "delay":
        time.sleep(request["milliseconds"] / 1000)
        return {"id": request["id"], "output": None}
    try:
        return {"id": request["id"], "output": capabilities.call(request["capability"], request["input"])}
    except CapabilityError as failure:
        refusals.append(Refusal(failure.kind, failure.message))
        return {"id": request["id"], "error": {"kind": failure.kind, "message": failure.message}}


def run_program(program: Program, inputs: Mapping[str, object], capabilities: Capabilities) -> Outcome:
    refusals: list[Refusal] = []
    with Kernel() as kernel, ThreadPoolExecutor(max_workers=MAX_WORKERS, thread_name_prefix="sx-repomod") as pool:
        reply = kernel.send(
            {
                "type": "start",
                "program": program.source,
                "declarations": program.declarations,
                "allow": sorted(program.allow),
                "inputs": dict(inputs),
            }
        )
        while reply["type"] == "requests":
            responses = list(pool.map(lambda r: answer(capabilities, r, refusals), reply["requests"]))
            reply = kernel.send({"type": "respond", "responses": responses})
    if reply["type"] == "rejected":
        raise RejectedProgramError(reply["rendered"])
    if reply["type"] == "completed":
        return Succeeded(reply["value"])
    assert reply["type"] == "failed"
    failure = reply["failure"]
    for refusal in refusals:
        if failure["message"] == f"A tool call failed with {refusal.kind}: {refusal.message}":
            return Errored(refusal.message)
    return Errored(f"{failure['kind']}: {failure['message']}")
