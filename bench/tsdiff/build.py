from __future__ import annotations

import json
import random
import tempfile
from pathlib import Path

from .case import Case, Json
from .compare import ROOT, shim_raw
from .generate import DECLARATIONS, WRITES, World, write_program
from .mutants import mutate
from .optionals import with_optional_nulls

SCRATCH = ROOT / ".work" / "tmp" / "tsdiff"
LEARNING_ROUNDS = 80


def declarations() -> Json:
    return json.loads(DECLARATIONS.read_text())


def tool_specs(declared: Json) -> list[dict[str, str]]:
    return [{"id": c["id"], "effect": c["effect"]} for c in declared["capabilities"]]


def without_nulls(value: Json) -> Json:
    return {k: v for k, v in value.items() if v is not None} if isinstance(value, dict) else value


def learn_responses(program: str, world: World, specs: list[dict[str, str]], declared: Json) -> list[Json]:
    inputs = {c["id"]: c["input"] for c in declared["capabilities"]}
    SCRATCH.mkdir(parents=True, exist_ok=True)
    responses: list[Json] = []
    with tempfile.TemporaryDirectory(dir=SCRATCH) as scratch:
        directory = Path(scratch)
        (directory / "program.ts").write_text(program)
        (directory / "tools.json").write_text(json.dumps(specs))
        for _ in range(LEARNING_ROUNDS):
            (directory / "responses.json").write_text(json.dumps(responses))
            raw = shim_raw(directory / "program.ts", directory / "tools.json", directory / "responses.json")
            error = raw.get("error")
            if error is None or error["class"] != "unexpected-call":
                return responses
            held = next(
                (
                    r
                    for r in responses
                    if r["capability"] == error["capability"] and without_nulls(r["input"]) == error["input"]
                ),
                None,
            )
            if held is not None:
                if "repeated-write" not in error["message"]:
                    return responses
                held["results"] += [held["results"][-1]] * len(held["results"])
                continue
            capability = error["capability"]
            results = world.results(capability, error["input"], capability in WRITES)
            expanded = with_optional_nulls(error["input"], inputs[capability], declared.get("types", {}))
            responses.append({"capability": capability, "input": expanded, "results": results})
    return responses


def generated_case(seed: int) -> Case:
    declared = declarations()
    specs = tool_specs(declared)
    program = write_program(seed)
    world = World(random.Random(seed ^ 0x5EED), seed)
    responses = learn_responses(program, world, specs, declared)
    return Case(
        name=f"gen-{seed}", program=program, declarations=declared, responses=responses, tools=specs,
        allow=tuple(s["id"] for s in specs), meta={"source": "generated", "seed": seed},
    )


def mutant_case(seed: int) -> Case:
    base = generated_case(seed)
    mutated, mutant = mutate(base.program, random.Random(seed ^ 0xBAD))
    return Case(
        name=f"mut-{seed}", program=mutated, declarations=base.declarations, responses=base.responses, tools=base.tools,
        allow=base.allow, meta={"source": "mutant", "seed": seed, "mutant": mutant.name, "expect_code": mutant.code},
    )
