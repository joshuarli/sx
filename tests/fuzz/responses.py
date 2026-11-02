from __future__ import annotations

import base64
import json
import random
from dataclasses import dataclass, field
from typing import Any

from sx.machine import CallRequest, DelayRequest, Output, Refusal, Response
from sx.registry import Effect, Registry
from sx.types import (
    EnumType,
    ListType,
    MapType,
    NominalType,
    OptionType,
    Primitive,
    RecordType,
    ResultType,
    TupleType,
    Type,
)

WORDS = ("a", "b", "c", "d", "ab", "x", "key", "name", "id", "zed", "", "A", "é", "😀", "￿", "a b", "done", "pending")
INTS = (0, 1, -1, 2, 3, 4, 5, 7, 10, 11, 100, 2**31, 2**53, 2**53 + 1, 2**62, 2**63 - 1, -(2**63), 3037000500)
FLOATS = (0.0, -0.0, 1.0, -1.0, 0.5, 1.5, 2.5, -2.5, 0.1, 21.0, 1e21, 1e-7, 1e300, 5e-324, 123456789012345680000.0, 3.0)
KINDS = ("Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed")
Json = Any


def value(t: Type, rng: random.Random, depth: int = 0) -> Json:
    match t:
        case Primitive(name="Bool"):
            return rng.random() < 0.5
        case Primitive(name="Int"):
            return rng.choice(INTS) if rng.random() < 0.3 else rng.randint(-3, 12)
        case Primitive(name="Float"):
            roll = rng.random()
            if roll < 0.25:
                return rng.choice(FLOATS)
            if roll < 0.4:
                return rng.randint(-5, 30)
            return rng.randint(-40, 40) / rng.choice([1, 2, 4, 10])
        case Primitive(name="String"):
            return rng.choice(WORDS)
        case Primitive(name="Bytes"):
            return base64.b64encode(bytes(rng.randrange(256) for _ in range(rng.randint(0, 6)))).decode("ascii")
        case Primitive(name="Unit"):
            return None
        case ListType(element=element):
            size = rng.choice([0, 1, 1, 2, 2, 3, 3, 4, 5, 8, 12]) if depth < 3 else rng.randint(0, 2)
            return [value(element, rng, depth + 1) for _ in range(size)]
        case TupleType(items=items):
            return [value(item, rng, depth + 1) for item in items]
        case OptionType(inner=inner):
            return None if rng.random() < 0.35 else value(inner, rng, depth + 1)
        case ResultType(ok=ok, err=err):
            if rng.random() < 0.5:
                return {"ok": value(ok, rng, depth + 1)}
            return {"err": value(err, rng, depth + 1)}
        case RecordType(fields=fields, open=open_record):
            out = {name: value(ft, rng, depth + 1) for name, ft in fields}
            if open_record and rng.random() < 0.6:
                out["extra"] = rng.choice([1, "x", None, [1, 2], {"k": 1}])
            return out
        case MapType(value=inner):
            keys = rng.sample(WORDS, rng.choice([0, 1, 2, 3, 4]))
            return {key: value(inner, rng, depth + 1) for key in keys}
        case EnumType(variants=variants):
            variant, payload = rng.choice(variants)
            if payload is None:
                return variant
            return {"tag": variant, "value": value(payload, rng, depth + 1)}
        case NominalType(base=base):
            return value(base, rng, depth)
    raise AssertionError(t)


def corrupt(t: Type, good: Json, rng: random.Random) -> Json:
    strategies = [
        lambda: None,
        lambda: "x",
        lambda: 1.5,
        lambda: True,
        list,
        dict,
        lambda: 2**63,
        lambda: -(2**63) - 1,
        lambda: 1.0,
        lambda: 1e400 if False else 1e300,
        lambda: [good],
        lambda: {"extra": good},
    ]
    if isinstance(good, dict) and good:
        key = rng.choice(list(good))
        dropped = {k: v for k, v in good.items() if k != key}
        extra = {**good, "extra": 1}
        renamed = {("x" + k): v for k, v in good.items()}
        strategies += [lambda: dropped, lambda: extra, lambda: renamed]
    if isinstance(good, list) and good:
        strategies += [lambda: good[:-1], lambda: [*good, good[0]]]
    if isinstance(t, EnumType):
        strategies += [lambda: "Bogus", lambda: {"tag": "Bogus", "value": 1}, lambda: {"tag": "Empty"}]
    if isinstance(t, Primitive) and t.name == "Bytes":
        strategies += [lambda: "AA", lambda: "AB==", lambda: "a-_=", lambda: " QQ==", lambda: "QQ==\n"]
    if isinstance(t, Primitive) and t.name == "Int":
        strategies += [lambda: 3.0, lambda: 1e2, lambda: "3"]
    return rng.choice(strategies)()


@dataclass(slots=True)
class Plan:
    error_rate: float = 0.15
    corrupt_rate: float = 0.03
    extend_rate: float = 0.5


@dataclass(slots=True)
class Entry:
    capability: str
    input: Json
    results: list[dict[str, Any]] = field(default_factory=list)
    served: int = 0
    frozen: bool = False


@dataclass(slots=True)
class ScriptHost:
    registry: Registry
    seed: str
    plan: Plan
    entries: list[Entry] = field(default_factory=list)

    def result(self, cap_id: str, payload: Json, index: int) -> dict[str, Any]:
        rng = random.Random(f"{self.seed}:{cap_id}:{json.dumps(payload, sort_keys=True)}:{index}")
        if rng.random() < self.plan.error_rate:
            kind = rng.choice(KINDS) if rng.random() > 0.01 else "Bogus"
            message = rng.choice(["", "boom", "retry later", "é😀", "x" * 40])
            return {"error": {"kind": kind, "message": message}}
        capability = self.registry.capability(cap_id)
        assert capability is not None
        if cap_id == "job.state":
            return {"output": rng.choice(["pending", "pending", "running", "done"])}
        good = value(capability.output, rng)
        if rng.random() < self.plan.corrupt_rate:
            return {"output": corrupt(capability.output, good, rng)}
        return {"output": good}

    def answer(self, request: CallRequest | DelayRequest) -> Response:
        if isinstance(request, DelayRequest):
            return Output(None)
        entry = next(
            (e for e in self.entries if e.capability == request.capability and e.input == request.input), None
        )
        if entry is None:
            entry = Entry(request.capability, request.input)
            self.entries.append(entry)
        capability = self.registry.capability(request.capability)
        assert capability is not None
        key = json.dumps(request.input, sort_keys=True)
        rng = random.Random(f"{self.seed}:extend:{request.capability}:{key}:{entry.served}")
        if entry.served >= len(entry.results) and not entry.frozen:
            if capability.effect is Effect.WRITE or not entry.results or rng.random() < self.plan.extend_rate:
                entry.results.append(self.result(request.capability, request.input, entry.served))
            else:
                entry.frozen = True
        result = entry.results[min(entry.served, len(entry.results) - 1)]
        entry.served += 1
        if "error" in result:
            return Refusal(result["error"]["kind"], result["error"]["message"])
        return Output(result["output"])

    def fixture_responses(self) -> list[dict[str, Any]]:
        return [{"capability": e.capability, "input": e.input, "results": e.results} for e in self.entries]
