from __future__ import annotations

from dataclasses import dataclass
from typing import override


@dataclass(frozen=True, slots=True)
class Position:
    line: int
    column: int


@dataclass(frozen=True, slots=True)
class Span:
    start: Position
    end: Position

    @override
    def __str__(self) -> str:
        return f"{self.start.line}:{self.start.column}-{self.end.line}:{self.end.column}"


@dataclass(frozen=True, slots=True)
class Symbol:
    name: str
    span: Span


@dataclass(frozen=True, slots=True)
class Keyword:
    name: str
    span: Span


@dataclass(frozen=True, slots=True)
class IntLiteral:
    value: int
    span: Span


@dataclass(frozen=True, slots=True)
class FloatLiteral:
    value: float
    span: Span


@dataclass(frozen=True, slots=True)
class DurationLiteral:
    milliseconds: int
    span: Span


@dataclass(frozen=True, slots=True)
class StringLiteral:
    value: str
    span: Span


@dataclass(frozen=True, slots=True)
class BoolLiteral:
    value: bool
    span: Span


@dataclass(frozen=True, slots=True)
class ListLiteral:
    items: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class RecordField:
    key: Keyword
    value: Node


@dataclass(frozen=True, slots=True)
class RecordLiteral:
    fields: tuple[RecordField, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class Form:
    items: tuple[Node, ...]
    span: Span


type Node = (
    Symbol | Keyword | IntLiteral | FloatLiteral | DurationLiteral | StringLiteral | BoolLiteral | ListLiteral
    | RecordLiteral | Form
)


def show(node: Node) -> str:
    match node:
        case Symbol(name=name):
            return name
        case Keyword(name=name):
            return f":{name}"
        case IntLiteral(value=value):
            return str(value)
        case FloatLiteral(value=value):
            return repr(value)
        case DurationLiteral(milliseconds=milliseconds):
            return show_duration(milliseconds)
        case BoolLiteral(value=value):
            return "true" if value else "false"
        case StringLiteral(value=value):
            return _show_string(value)
        case ListLiteral(items=items):
            return "[" + " ".join(show(item) for item in items) + "]"
        case RecordLiteral(fields=fields):
            return "{" + " ".join(f":{field.key.name} {show(field.value)}" for field in fields) + "}"
        case Form(items=items):
            return "(" + " ".join(show(item) for item in items) + ")"


DURATION_UNITS = (("h", 3_600_000), ("m", 60_000), ("s", 1_000), ("ms", 1))


def show_duration(milliseconds: int) -> str:
    for unit, size in DURATION_UNITS:
        if milliseconds % size == 0 and (milliseconds != 0 or unit == "ms"):
            return f"{milliseconds // size}{unit}"
    raise AssertionError(milliseconds)


_STRING_ESCAPES = {'"': '\\"', "\\": "\\\\", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def _show_string(value: str) -> str:
    parts: list[str] = []
    for char in value:
        if char in _STRING_ESCAPES:
            parts.append(_STRING_ESCAPES[char])
        elif not char.isprintable():
            parts.append(f"\\u{{{ord(char):X}}}")
        else:
            parts.append(char)
    return '"' + "".join(parts) + '"'
