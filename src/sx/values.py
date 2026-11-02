from __future__ import annotations

import json
from collections.abc import Iterable, Iterator
from dataclasses import dataclass, field
from typing import override

from sx.types import (
    BOOL,
    CALL_ERROR,
    INT,
    STRING,
    UNIT,
    EnumType,
    ListType,
    MapType,
    NominalType,
    OptionType,
    RecordType,
    ResultType,
    TupleType,
    Type,
    Variable,
    show_type,
)

INT_MIN = -(2**63)
INT_MAX = 2**63 - 1


@dataclass(frozen=True, slots=True)
class UnitValue:
    pass


UNIT_VALUE = UnitValue()


@dataclass(frozen=True, slots=True)
class ListValue:
    items: tuple[Value, ...]


@dataclass(frozen=True, slots=True)
class TupleValue:
    items: tuple[Value, ...]


@dataclass(frozen=True, slots=True)
class RecordValue:
    fields: tuple[tuple[str, Value], ...]
    _index: dict[str, Value] = field(default_factory=dict, compare=False, hash=False, repr=False)

    def __post_init__(self) -> None:
        self._index.update(self.fields)

    def get(self, name: str) -> Value:
        return self._index[name]

    @override
    def __eq__(self, other: object) -> bool:
        return isinstance(other, RecordValue) and self._index == other._index

    @override
    def __hash__(self) -> int:
        return hash(frozenset(self._index.items()))


@dataclass(frozen=True, slots=True)
class MapValue:
    entries: tuple[tuple[str, Value], ...]
    _index: dict[str, Value] = field(default_factory=dict, compare=False, hash=False, repr=False)

    def __post_init__(self) -> None:
        self._index.update(self.entries)

    def get(self, key: str) -> Value | None:
        return self._index.get(key)

    def has(self, key: str) -> bool:
        return key in self._index


def make_map(pairs: Iterable[tuple[str, Value]]) -> MapValue:
    return MapValue(tuple(sorted(dict(pairs).items())))


@dataclass(frozen=True, slots=True)
class SomeValue:
    value: Value


@dataclass(frozen=True, slots=True)
class NoneValue:
    pass


NONE_VALUE = NoneValue()


@dataclass(frozen=True, slots=True)
class OkValue:
    value: Value


@dataclass(frozen=True, slots=True)
class ErrValue:
    value: Value


@dataclass(frozen=True, slots=True)
class VariantValue:
    enum: str
    variant: str
    payload: Value | None = None


type Value = (
    bool | int | str | UnitValue | ListValue | TupleValue | RecordValue | MapValue | SomeValue | NoneValue | OkValue
    | ErrValue | VariantValue
)


def call_error(kind: str, message: str) -> RecordValue:
    return RecordValue((("kind", VariantValue("CallErrorKind", kind)), ("message", message)))


def size(value: Value) -> int:
    match value:
        case str():
            return 1 + len(value) // 32
        case ListValue(items=items) | TupleValue(items=items):
            return 1 + sum(size(item) for item in items)
        case RecordValue(fields=fields):
            return 1 + sum(size(v) for _, v in fields)
        case MapValue(entries=entries):
            return 1 + sum(size(v) for _, v in entries)
        case SomeValue(value=inner) | OkValue(value=inner) | ErrValue(value=inner):
            return 1 + size(inner)
        case VariantValue(payload=payload):
            return 1 + (0 if payload is None else size(payload))
        case _:
            return 1


@dataclass(frozen=True, slots=True)
class DecodeError(ValueError):
    path: str
    expected: str
    found: str

    @override
    def __str__(self) -> str:
        return f"{self.path}: expected {self.expected}, found {self.found}"


def _json_kind(value: object) -> str:
    match value:
        case None:
            return "null"
        case bool():
            return "a boolean"
        case int():
            return "an integer"
        case float():
            return "a number with a fraction"
        case str():
            return "a string"
        case list():
            return "an array"
        case dict():
            return "an object"
        case _:
            return type(value).__name__


def _json_object(value: object) -> dict[str, object] | None:
    if not isinstance(value, dict):
        return None
    if not all(isinstance(key, str) for key in value):
        return None
    return {key: item for key, item in value.items() if isinstance(key, str)}


def decode(value: object, t: Type, path: str = "$") -> Value:
    def fail(expected: str) -> DecodeError:
        return DecodeError(path, expected, _json_kind(value))

    match t:
        case _ if t == BOOL:
            if not isinstance(value, bool):
                raise fail("Bool")
            return value
        case _ if t == INT:
            if isinstance(value, bool) or not isinstance(value, int):
                raise fail("Int")
            if not INT_MIN <= value <= INT_MAX:
                raise DecodeError(path, "Int", "an integer outside the signed 64-bit range")
            return value
        case _ if t == STRING:
            if not isinstance(value, str):
                raise fail("String")
            return value
        case _ if t == UNIT:
            if value is not None:
                raise fail("Unit (null)")
            return UNIT_VALUE
        case ListType(element=element):
            if not isinstance(value, list):
                raise fail(show_type(t))
            return ListValue(tuple(decode(item, element, f"{path}[{i}]") for i, item in enumerate(value)))
        case TupleType(items=items):
            if not isinstance(value, list) or len(value) != len(items):
                raise fail(f"{show_type(t)} as an array of {len(items)}")
            return TupleValue(
                tuple(decode(v, it, f"{path}[{i}]") for i, (v, it) in enumerate(zip(value, items, strict=True))),
            )
        case OptionType(inner=inner):
            return NONE_VALUE if value is None else SomeValue(decode(value, inner, path))
        case ResultType(ok=ok, err=err):
            obj = _json_object(value)
            if obj is not None and set(obj) == {"ok"}:
                return OkValue(decode(obj["ok"], ok, f"{path}.ok"))
            if obj is not None and set(obj) == {"err"}:
                return ErrValue(decode(obj["err"], err, f"{path}.err"))
            raise fail('{"ok": ...} or {"err": ...}')
        case MapType(value=value_type):
            obj = _json_object(value)
            if obj is None:
                raise fail(show_type(t))
            return MapValue(
                tuple((key, decode(obj[key], value_type, f"{path}[{json.dumps(key)}]")) for key in sorted(obj)),
            )
        case RecordType(fields=fields):
            obj = _json_object(value)
            if obj is None:
                raise fail(show_type(t))
            names = [name for name, _ in fields]
            missing = [name for name in names if name not in obj]
            if missing:
                raise DecodeError(f"{path}.{missing[0]}", show_type(dict(fields)[missing[0]]), "no field")
            extra = sorted(set(obj) - set(names))
            if extra:
                raise DecodeError(
                    f"{path}.{extra[0]}", "no field, because the record type does not declare it", "a field",
                )
            return RecordValue(tuple((name, decode(obj[name], ft, f"{path}.{name}")) for name, ft in fields))
        case EnumType(name=name, variants=variants):
            if isinstance(value, str):
                for variant, payload in variants:
                    if variant == value and payload is None:
                        return VariantValue(name, variant)
            obj = _json_object(value)
            if obj is not None and set(obj) == {"tag", "value"}:
                for variant, payload in variants:
                    if variant == obj["tag"] and payload is not None:
                        return VariantValue(name, variant, decode(obj["value"], payload, f"{path}.value"))
            choices = " | ".join(v if p is None else f"{{tag: {v}}}" for v, p in variants)
            raise fail(f"{name} ({choices})")
        case NominalType(base=base):
            return decode(value, base, path)
        case Variable():
            raise DecodeError(path, "a known type", "a type the checker could not resolve")
        case _:
            raise fail(show_type(t))


def encode(value: Value, t: Type) -> object:
    match value:
        case bool() | int() | str():
            return value
        case UnitValue() | NoneValue():
            return None
        case ListValue(items=items):
            element = t.element if isinstance(t, ListType) else t
            return [encode(item, element) for item in items]
        case TupleValue(items=items):
            types = t.items if isinstance(t, TupleType) else (t,) * len(items)
            return [encode(item, it) for item, it in zip(items, types, strict=True)]
        case SomeValue(value=inner):
            return encode(inner, t.inner if isinstance(t, OptionType) else t)
        case OkValue(value=inner):
            return {"ok": encode(inner, t.ok if isinstance(t, ResultType) else t)}
        case ErrValue(value=inner):
            return {"err": encode(inner, t.err if isinstance(t, ResultType) else CALL_ERROR)}
        case MapValue(entries=entries):
            value_type = t.value if isinstance(t, MapType) else t
            return {key: encode(v, value_type) for key, v in entries}
        case RecordValue(fields=fields):
            types = dict(t.fields) if isinstance(t, RecordType) else {}
            return {name: encode(v, types.get(name, t)) for name, v in fields}
        case VariantValue(variant=variant, payload=payload):
            if payload is None:
                return variant
            payload_type = t.payload(variant) if isinstance(t, EnumType) else t
            return {"tag": variant, "value": encode(payload, payload_type or t)}


def iterate(value: Value) -> Iterator[Value]:
    assert isinstance(value, ListValue)
    return iter(value.items)


def show_value(value: Value) -> str:
    match value:
        case bool():
            return "true" if value else "false"
        case int():
            return str(value)
        case str():
            escaped = value.replace("\\", "\\\\").replace('"', '\\"').replace("\n", "\\n").replace("\t", "\\t")
            return f'"{escaped.replace(chr(13), chr(92) + "r")}"'
        case UnitValue():
            return "unit"
        case ListValue(items=items) | TupleValue(items=items):
            return "[" + " ".join(show_value(item) for item in items) + "]"
        case RecordValue(fields=fields):
            return "{" + " ".join(f":{name} {show_value(v)}" for name, v in fields) + "}"
        case MapValue(entries=entries):
            pairs = " ".join(f"[{show_value(key)} {show_value(v)}]" for key, v in entries)
            return f"(into-map [{pairs}])"
        case SomeValue(value=inner):
            return f"(some {show_value(inner)})"
        case NoneValue():
            return "none"
        case OkValue(value=inner):
            return f"(ok {show_value(inner)})"
        case ErrValue(value=inner):
            return f"(err {show_value(inner)})"
        case VariantValue(variant=variant, payload=payload):
            return variant if payload is None else f"({variant} {show_value(payload)})"
