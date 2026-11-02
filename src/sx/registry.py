from __future__ import annotations

import re
from collections.abc import Callable, Mapping
from dataclasses import dataclass, field, replace
from enum import StrEnum
from typing import Any, cast

from sx.types import EnumType, RecordType, Type, TypeSyntaxError, parse_type
from sx.values import Value

CORE_ID = re.compile(r"[a-z][a-z0-9]*(-[a-z0-9]+)*")
MEMBER = re.compile(r"[a-z][a-z0-9]*(-[a-z0-9]+)*\??")


class Effect(StrEnum):
    READ = "read"
    WRITE = "write"


@dataclass(frozen=True, slots=True)
class Example:
    program: str
    result: str


@dataclass(frozen=True, slots=True)
class Function:
    name: str
    params: tuple[Type, ...]
    output: Type
    description: str
    example: Example | None = None
    implementation: Callable[..., Value] | None = None


@dataclass(frozen=True, slots=True)
class Capability:
    id: str
    effect: Effect
    input: Type
    output: Type
    description: str
    idempotent: bool = False


class Assurance(StrEnum):
    UNVERIFIED = "unverified"
    CONFORMANCE_TESTED = "conformance-tested"
    SPECIFIED = "specified"
    PROVED_PROPERTIES = "proved-properties"


@dataclass(frozen=True, slots=True)
class Evidence:
    level: Assurance
    references: tuple[str, ...]


@dataclass(frozen=True, slots=True)
class Core:
    id: str
    version: str
    functions: tuple[Function, ...]
    types: Mapping[str, Type] = field(default_factory=dict)
    evidence: tuple[Evidence, ...] = ()

    @property
    def assurance(self) -> Mapping[Assurance, tuple[str, ...]]:
        return {Assurance.UNVERIFIED: (), **{e.level: e.references for e in self.evidence}}


class RegistryError(ValueError):
    pass


class Registry:
    def __init__(self) -> None:
        self._functions: dict[str, Function] = {}
        self._capabilities: dict[str, Capability] = {}
        self._types: dict[str, Type] = {}
        self._cores: dict[str, Core] = {}

    def add_core(self, core: Core) -> None:
        if not CORE_ID.fullmatch(core.id):
            raise RegistryError(f"core id {core.id!r} is not lowercase words joined by -")
        if core.id in self._cores:
            raise RegistryError(f"core {core.id!r} is already registered")
        self._check_evidence(core)
        for name in core.types:
            self._add_type(name, core.types[name], owner=core.id)
        for function in core.functions:
            namespace, _, member = function.name.partition(".")
            if namespace != core.id or not MEMBER.fullmatch(member):
                raise RegistryError(f"function {function.name!r} must be named {core.id}.NAME")
            if function.name in self._functions:
                raise RegistryError(f"function {function.name!r} is already registered")
            if not function.description.strip():
                raise RegistryError(f"function {function.name!r} needs a one-line description")
            if function.example is None:
                raise RegistryError(f"function {function.name!r} needs an example with its result")
            self._functions[function.name] = function
        self._cores[core.id] = core

    @staticmethod
    def _check_evidence(core: Core) -> None:
        levels = [e.level for e in core.evidence]
        if Assurance.UNVERIFIED in levels:
            raise RegistryError(f"core {core.id!r} declares unverified evidence; registering the interface is that")
        if len(set(levels)) != len(levels):
            raise RegistryError(f"core {core.id!r} declares one level of evidence twice")
        for evidence in core.evidence:
            if not evidence.references:
                raise RegistryError(f"core {core.id!r} claims {evidence.level.value} without naming its evidence")
        if Assurance.PROVED_PROPERTIES in levels and Assurance.SPECIFIED not in levels:
            raise RegistryError(f"core {core.id!r} claims proved properties of a model it does not specify")

    def add_capability(self, capability: Capability) -> None:
        namespace, _, member = capability.id.partition(".")
        if not CORE_ID.fullmatch(namespace) or not MEMBER.fullmatch(member):
            raise RegistryError(f"capability id {capability.id!r} must be NAMESPACE.NAME")
        if capability.id in self._capabilities:
            raise RegistryError(f"capability {capability.id!r} is already registered")
        if not capability.description.strip():
            raise RegistryError(f"capability {capability.id!r} needs a one-line description")
        if capability.effect is Effect.READ and capability.idempotent:
            raise RegistryError(f"capability {capability.id!r} is a read, so idempotent does not apply")
        self._capabilities[capability.id] = capability

    def add_type(self, name: str, t: Type) -> None:
        self._add_type(name, t, owner=None)

    def _add_type(self, name: str, t: Type, owner: str | None) -> None:
        if owner is not None and not name.startswith(f"{owner}."):
            raise RegistryError(f"type {name!r} of core {owner!r} must be named {owner}.NAME")
        if name in self._types and self._types[name] != t:
            raise RegistryError(f"type {name!r} is already registered with a different definition")
        self._types[name] = t

    def function(self, name: str) -> Function | None:
        return self._functions.get(name)

    def core(self, core_id: str) -> Core | None:
        return self._cores.get(core_id)

    @property
    def cores(self) -> Mapping[str, Core]:
        return self._cores

    def capability(self, capability_id: str) -> Capability | None:
        return self._capabilities.get(capability_id)

    @property
    def functions(self) -> Mapping[str, Function]:
        return self._functions

    @property
    def capabilities(self) -> Mapping[str, Capability]:
        return self._capabilities

    @property
    def types(self) -> Mapping[str, Type]:
        return self._types

    def parse(self, text: str) -> Type:
        try:
            return parse_type(text, self._types)
        except TypeSyntaxError as error:
            raise RegistryError(str(error)) from error


def with_name(name: str, t: Type) -> Type:
    return replace(t, name=name) if isinstance(t, RecordType) else t


def load_declarations(registry: Registry, declarations: Mapping[str, Any]) -> None:
    enums = cast(Mapping[str, Mapping[str, str | None]], declarations.get("enums", {}))
    for name, variants in enums.items():
        registry.add_type(
            name,
            EnumType(
                name,
                tuple(
                    (variant, None if payload is None else registry.parse(payload))
                    for variant, payload in variants.items()
                ),
            ),
        )
    pending = dict(declarations.get("types", {}))
    while pending:
        progressed = False
        for name, definition in list(pending.items()):
            try:
                registry.add_type(name, with_name(name, registry.parse(definition)))
            except RegistryError:
                continue
            del pending[name]
            progressed = True
        if not progressed:
            name, definition = next(iter(pending.items()))
            registry.add_type(name, with_name(name, registry.parse(definition)))
    for entry in declarations.get("capabilities", []):
        registry.add_capability(
            Capability(
                id=entry["id"],
                effect=Effect(entry["effect"]),
                input=registry.parse(entry["input"]),
                output=registry.parse(entry["output"]),
                description=entry["description"],
                idempotent=bool(entry.get("idempotent", False)),
            )
        )
