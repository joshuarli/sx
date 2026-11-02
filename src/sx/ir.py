from __future__ import annotations

from dataclasses import dataclass

from sx.registry import Capability, Function
from sx.syntax import Span
from sx.types import Type
from sx.values import Value


@dataclass(frozen=True, slots=True)
class BindName:
    name: str


@dataclass(frozen=True, slots=True)
class Discard:
    pass


@dataclass(frozen=True, slots=True)
class BindTuple:
    items: tuple[Binder, ...]


type Binder = BindName | Discard | BindTuple


@dataclass(frozen=True, slots=True)
class MatchSome:
    inner: MatchPattern


@dataclass(frozen=True, slots=True)
class MatchNone:
    pass


@dataclass(frozen=True, slots=True)
class MatchOk:
    inner: MatchPattern


@dataclass(frozen=True, slots=True)
class MatchErr:
    inner: MatchPattern


@dataclass(frozen=True, slots=True)
class MatchVariant:
    variant: str
    inner: MatchPattern | None


@dataclass(frozen=True, slots=True)
class MatchAny:
    binder: Binder


type MatchPattern = MatchSome | MatchNone | MatchOk | MatchErr | MatchVariant | MatchAny


@dataclass(frozen=True, slots=True)
class ToSome:
    inner: Conversion | None


@dataclass(frozen=True, slots=True)
class EachField:
    fields: tuple[tuple[str, Conversion | None], ...]


@dataclass(frozen=True, slots=True)
class EachItem:
    items: tuple[Conversion | None, ...]


@dataclass(frozen=True, slots=True)
class EachElement:
    element: Conversion


@dataclass(frozen=True, slots=True)
class VariantName:
    pass


type Conversion = ToSome | EachField | EachItem | EachElement | VariantName


@dataclass(frozen=True, slots=True)
class Literal:
    value: Value
    type: Type
    span: Span


@dataclass(frozen=True, slots=True)
class Variable:
    name: str
    span: Span


@dataclass(frozen=True, slots=True)
class Let:
    binder: Binder
    value: Node
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class LocalFunction:
    name: str
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class If:
    condition: Node
    then: Node
    otherwise: Node
    span: Span


@dataclass(frozen=True, slots=True)
class Match:
    scrutinee: Node
    clauses: tuple[tuple[MatchPattern, Node], ...]
    span: Span


@dataclass(frozen=True, slots=True)
class Lambda:
    params: tuple[Binder, ...]
    param_types: tuple[Type, ...]
    result: Type
    body: Node
    captured: str | None
    span: Span


@dataclass(frozen=True, slots=True)
class ApplyLocal:
    name: str
    function: Lambda
    args: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class Prelude:
    op: str
    args: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class CoreCall:
    function: Function
    args: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class Call:
    capability: Capability
    argument: Node
    span: Span


@dataclass(frozen=True, slots=True)
class Get:
    record: Node
    field: str
    span: Span


@dataclass(frozen=True, slots=True)
class MakeRecord:
    fields: tuple[tuple[str, Node], ...]
    span: Span


@dataclass(frozen=True, slots=True)
class MakeList:
    items: tuple[Node, ...]
    element: Type
    span: Span


@dataclass(frozen=True, slots=True)
class MakeTuple:
    items: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class MakeBracket:
    items: tuple[Node, ...]
    shape: Type
    span: Span


@dataclass(frozen=True, slots=True)
class MakeEmptyBrace:
    shape: Type
    span: Span


@dataclass(frozen=True, slots=True)
class Wrap:
    kind: str
    value: Node
    type: Type
    span: Span


@dataclass(frozen=True, slots=True)
class MakeVariant:
    enum: str
    variant: str
    payload: Node | None
    span: Span


@dataclass(frozen=True, slots=True)
class Coerce:
    value: Node
    conversion: Conversion
    span: Span


@dataclass(frozen=True, slots=True)
class Attempt:
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class Retry:
    delays: tuple[int, ...]
    kinds: frozenset[str]
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class WaitUntil:
    intervals: tuple[int, ...]
    predicate: Lambda
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class Loop:
    maximum: int
    binders: tuple[Binder, ...]
    initial: tuple[Node, ...]
    body: Node
    span: Span


@dataclass(frozen=True, slots=True)
class Recur:
    args: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class Par:
    branches: tuple[Node, ...]
    span: Span


@dataclass(frozen=True, slots=True)
class ParMap:
    limit: int
    function: Lambda
    items: Node
    span: Span


type Node = (
    Literal | Variable | Let | LocalFunction | If | Match | Lambda | ApplyLocal | Prelude | CoreCall | Call | Get
    | MakeRecord | MakeList | MakeTuple | MakeBracket | MakeEmptyBrace | Wrap | MakeVariant | Coerce | Attempt | Retry
    | WaitUntil | Loop | Recur | Par | ParMap
)


@dataclass(frozen=True, slots=True)
class Program:
    body: Node
    result: Type
    capabilities: frozenset[str]
    inputs: frozenset[str]
