from __future__ import annotations

import re
from collections.abc import Mapping
from dataclasses import dataclass, field


@dataclass(frozen=True, slots=True)
class Primitive:
    name: str


BOOL = Primitive("Bool")
INT = Primitive("Int")
STRING = Primitive("String")
UNIT = Primitive("Unit")


@dataclass(frozen=True, slots=True)
class ListType:
    element: Type


@dataclass(frozen=True, slots=True)
class MapType:
    value: Type


@dataclass(frozen=True, slots=True)
class TupleType:
    items: tuple[Type, ...]


@dataclass(frozen=True, slots=True)
class OptionType:
    inner: Type


@dataclass(frozen=True, slots=True)
class ResultType:
    ok: Type
    err: Type


@dataclass(frozen=True, slots=True)
class RecordType:
    fields: tuple[tuple[str, Type], ...]
    name: str | None = field(default=None, compare=False)

    def field(self, name: str) -> Type | None:
        for field_name, field_type in self.fields:
            if field_name == name:
                return field_type
        return None

    @property
    def names(self) -> frozenset[str]:
        return frozenset(name for name, _ in self.fields)


@dataclass(frozen=True, slots=True)
class EnumType:
    name: str
    variants: tuple[tuple[str, Type | None], ...]

    def payload(self, variant: str) -> Type | None:
        for name, payload in self.variants:
            if name == variant:
                return payload
        raise KeyError(variant)

    def has(self, variant: str) -> bool:
        return any(name == variant for name, _ in self.variants)


@dataclass(frozen=True, slots=True)
class NominalType:
    name: str
    base: Type


@dataclass(frozen=True, slots=True)
class Variable:
    id: int


@dataclass(frozen=True, slots=True)
class Never:
    pass


NEVER = Never()

type Type = (
    Primitive | ListType | MapType | TupleType | OptionType | ResultType | RecordType | EnumType | NominalType
    | Variable | Never
)

CALL_ERROR_KINDS = (
    "Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed",
)
TRANSIENT_KINDS = frozenset({"Timeout", "Unavailable", "RateLimited"})
CALL_ERROR_KIND = EnumType("CallErrorKind", tuple((kind, None) for kind in CALL_ERROR_KINDS))
CALL_ERROR = RecordType((("kind", CALL_ERROR_KIND), ("message", STRING)))


def show_type(t: Type) -> str:
    match t:
        case Primitive(name=name):
            return name
        case ListType(element=element):
            return f"List<{show_type(element)}>"
        case MapType(value=value):
            return f"Map<String, {show_type(value)}>"
        case TupleType(items=items):
            return f"Tuple<{', '.join(show_type(item) for item in items)}>"
        case OptionType(inner=inner):
            return f"Option<{show_type(inner)}>"
        case ResultType(ok=ok, err=err):
            return f"Result<{show_type(ok)}, {'CallError' if err == CALL_ERROR else show_type(err)}>"
        case RecordType(name=name):
            if t == CALL_ERROR:
                return "CallError"
            if name is not None:
                return name
            return show_definition(t)
        case EnumType(name=name) | NominalType(name=name):
            return name
        case Variable():
            return "?"
        case Never():
            return "Never"


def show_definition(t: Type) -> str:
    if isinstance(t, RecordType):
        return "Record {" + ", ".join(f"{name}: {show_type(ft)}" for name, ft in t.fields) + "}"
    return show_type(t)


class TypeSyntaxError(ValueError):
    pass


UNSUPPORTED = {
    "Float": "Sx v0 has no Float type",
    "Bytes": "Sx v0 has no Bytes type",
}
_TOKEN = re.compile(r"\s*(?:([A-Za-z][A-Za-z0-9_.-]*)|(.))")


def parse_type(text: str, named: Mapping[str, Type]) -> Type:
    tokens = [match.group(1) or match.group(2) for match in _TOKEN.finditer(text) if match.group(1) or match.group(2)]
    parser = _TypeParser(tokens, named, text)
    result = parser.type()
    if parser.at < len(tokens):
        raise TypeSyntaxError(f"unexpected {tokens[parser.at]!r} in type {text!r}")
    return result


class _TypeParser:
    def __init__(self, tokens: list[str], named: Mapping[str, Type], text: str) -> None:
        self.tokens = tokens
        self.named = named
        self.text = text
        self.at = 0

    def peek(self) -> str | None:
        return self.tokens[self.at] if self.at < len(self.tokens) else None

    def take(self, expected: str | None = None) -> str:
        token = self.peek()
        if token is None:
            raise TypeSyntaxError(f"type {self.text!r} ends early")
        if expected is not None and token != expected:
            raise TypeSyntaxError(f"expected {expected!r}, found {token!r} in type {self.text!r}")
        self.at += 1
        return token

    def arguments(self, count: int, name: str) -> list[Type]:
        self.take("<")
        args = [self.type()]
        while self.peek() == ",":
            self.take(",")
            args.append(self.type())
        self.take(">")
        if count >= 0 and len(args) != count:
            raise TypeSyntaxError(f"{name} takes {count} type argument{'s' if count != 1 else ''} in {self.text!r}")
        return args

    def type(self) -> Type:
        name = self.take()
        if name in UNSUPPORTED:
            raise TypeSyntaxError(f"{UNSUPPORTED[name]}: {self.text!r}")
        match name:
            case "Bool":
                return BOOL
            case "Int":
                return INT
            case "String":
                return STRING
            case "Unit":
                return UNIT
            case "CallError":
                return CALL_ERROR
            case "List":
                return ListType(self.arguments(1, name)[0])
            case "Map":
                key, value = self.arguments(2, name)
                if key != STRING:
                    raise TypeSyntaxError(
                        f"Map keys must be String, because JSON object keys are strings: {self.text!r}"
                    )
                return MapType(value)
            case "Option":
                return OptionType(self.arguments(1, name)[0])
            case "Result":
                ok, err = self.arguments(2, name)
                return ResultType(ok, err)
            case "Tuple":
                return TupleType(tuple(self.arguments(-1, name)))
            case "Record":
                return self.record()
        if name in self.named:
            return self.named[name]
        raise TypeSyntaxError(f"unknown type {name!r} in {self.text!r}")

    def record(self) -> RecordType:
        self.take("{")
        fields: list[tuple[str, Type]] = []
        while self.peek() != "}":
            field_name = self.take()
            self.take(":")
            if any(existing == field_name for existing, _ in fields):
                raise TypeSyntaxError(f"field {field_name!r} appears twice in {self.text!r}")
            fields.append((field_name, self.type()))
            if self.peek() == ",":
                self.take(",")
        self.take("}")
        return RecordType(tuple(fields))


def encodable_problem(t: Type) -> str | None:
    match t:
        case OptionType(inner=inner):
            if isinstance(inner, OptionType) or inner == UNIT:
                return f"{show_type(t)} has no JSON encoding, because none and (some x) would both be null"
            return encodable_problem(inner)
        case ListType(element=element):
            return encodable_problem(element)
        case MapType(value=value):
            if isinstance(value, OptionType):
                return f"{show_type(t)} has no JSON encoding, because a null value and a missing key would collide"
            return encodable_problem(value)
        case TupleType(items=items):
            return next((problem for item in items if (problem := encodable_problem(item)) is not None), None)
        case ResultType(ok=ok, err=err):
            return encodable_problem(ok) or encodable_problem(err)
        case RecordType(fields=fields):
            return next((problem for _, ft in fields if (problem := encodable_problem(ft)) is not None), None)
        case EnumType(variants=variants):
            return next(
                (
                    problem
                    for _, payload in variants
                    if payload is not None and (problem := encodable_problem(payload)) is not None
                ),
                None,
            )
        case NominalType(base=base):
            return encodable_problem(base)
        case _:
            return None
