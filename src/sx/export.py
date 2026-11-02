from __future__ import annotations

from collections.abc import Iterable, Mapping

from sx import ir
from sx.registry import Capability, Function
from sx.syntax import Span
from sx.types import (
    EnumType,
    ListType,
    MapType,
    Never,
    NominalType,
    OptionType,
    Primitive,
    RecordType,
    ResultType,
    TupleType,
    Type,
    Variable,
)
from sx.values import (
    ErrValue,
    ListValue,
    MapValue,
    NoneValue,
    OkValue,
    RecordValue,
    SomeValue,
    TupleValue,
    UnitValue,
    Value,
    VariantValue,
)

type Json = bool | int | str | list[Json] | dict[str, Json] | None


class _Exporter:
    def __init__(self) -> None:
        self.enums: dict[str, Json] = {}
        self.capabilities: dict[str, Json] = {}
        self.functions: dict[str, Json] = {}

    def type(self, t: Type) -> Json:
        match t:
            case Primitive(name=name):
                return name
            case ListType(element=element):
                return {"list": self.type(element)}
            case MapType(value=value):
                return {"map": self.type(value)}
            case TupleType(items=items):
                return {"tuple": [self.type(item) for item in items]}
            case OptionType(inner=inner):
                return {"option": self.type(inner)}
            case ResultType(ok=ok, err=err):
                return {"result": [self.type(ok), self.type(err)]}
            case RecordType(fields=fields, name=name):
                return {"record": [[field, self.type(ft)] for field, ft in fields], "name": name}
            case EnumType(name=name, variants=variants):
                if name not in self.enums:
                    self.enums[name] = None
                    self.enums[name] = [
                        [variant, None if payload is None else self.type(payload)] for variant, payload in variants
                    ]
                return {"enum": name}
            case NominalType(name=name, base=base):
                return {"nominal": name, "base": self.type(base)}
            case Variable(id=identifier):
                return {"var": identifier}
            case Never():
                return "Never"

    def value(self, value: Value) -> Json:
        match value:
            case bool():
                return {"bool": value}
            case int():
                return {"int": value}
            case str():
                return {"string": value}
            case UnitValue():
                return "unit"
            case NoneValue():
                return "none"
            case SomeValue(value=inner):
                return {"some": self.value(inner)}
            case OkValue(value=inner):
                return {"ok": self.value(inner)}
            case ErrValue(value=inner):
                return {"err": self.value(inner)}
            case ListValue(items=items):
                return {"list": [self.value(item) for item in items]}
            case TupleValue(items=items):
                return {"tuple": [self.value(item) for item in items]}
            case RecordValue(fields=fields):
                return {"record": [[name, self.value(item)] for name, item in fields]}
            case MapValue(entries=entries):
                return {"map": [[key, self.value(item)] for key, item in entries]}
            case VariantValue(enum=enum, variant=variant, payload=payload):
                return {
                    "variant": {
                        "enum": enum, "name": variant, "payload": None if payload is None else self.value(payload),
                    },
                }

    def binder(self, binder: ir.Binder) -> Json:
        match binder:
            case ir.BindName(name=name):
                return {"bind": name}
            case ir.Discard():
                return "discard"
            case ir.BindTuple(items=items):
                return {"tuple": [self.binder(item) for item in items]}

    def pattern(self, pattern: ir.MatchPattern) -> Json:
        match pattern:
            case ir.MatchSome(inner=inner):
                return {"some": self.pattern(inner)}
            case ir.MatchNone():
                return "none"
            case ir.MatchOk(inner=inner):
                return {"ok": self.pattern(inner)}
            case ir.MatchErr(inner=inner):
                return {"err": self.pattern(inner)}
            case ir.MatchVariant(variant=variant, inner=inner):
                return {"variant": variant, "inner": None if inner is None else self.pattern(inner)}
            case ir.MatchAny(binder=binder):
                return {"any": self.binder(binder)}

    def conversion(self, conversion: ir.Conversion | None) -> Json:
        match conversion:
            case None:
                return None
            case ir.ToSome(inner=inner):
                return {"to-some": self.conversion(inner)}
            case ir.EachField(fields=fields):
                return {"each-field": [[name, self.conversion(c)] for name, c in fields]}
            case ir.EachItem(items=items):
                return {"each-item": [self.conversion(c) for c in items]}
            case ir.EachElement(element=element):
                return {"each-element": self.conversion(element)}
            case ir.VariantName():
                return "variant-name"

    def capability(self, capability: Capability) -> str:
        self.capabilities[capability.id] = {
            "effect": capability.effect.value,
            "idempotent": capability.idempotent,
            "input": self.type(capability.input),
            "output": self.type(capability.output),
        }
        return capability.id

    def function(self, function: Function) -> str:
        self.functions[function.name] = {
            "params": [self.type(p) for p in function.params],
            "output": self.type(function.output),
        }
        return function.name

    def nodes(self, nodes: tuple[ir.Node, ...]) -> list[Json]:
        return [self.node(n) for n in nodes]

    def lam(self, node: ir.Lambda) -> dict[str, Json]:
        return {
            "params": [self.binder(b) for b in node.params],
            "param_types": [self.type(t) for t in node.param_types],
            "result": self.type(node.result),
            "body": self.node(node.body),
            "captured": node.captured,
        }

    def node(self, node: ir.Node) -> Json:
        fields: dict[str, Json]
        match node:
            case ir.Literal(value=value, type=t):
                fields = {"value": self.value(value), "type": self.type(t)}
            case ir.Variable(name=name):
                fields = {"name": name}
            case ir.Let(binder=binder, value=value, body=body):
                fields = {"binder": self.binder(binder), "value": self.node(value), "body": self.node(body)}
            case ir.LocalFunction(name=name, body=body):
                fields = {"name": name, "body": self.node(body)}
            case ir.If(condition=condition, then=then, otherwise=otherwise):
                fields = {"condition": self.node(condition), "then": self.node(then), "otherwise": self.node(otherwise)}
            case ir.Match(scrutinee=scrutinee, clauses=clauses):
                fields = {
                    "scrutinee": self.node(scrutinee),
                    "clauses": [[self.pattern(p), self.node(body)] for p, body in clauses],
                }
            case ir.Lambda():
                fields = self.lam(node)
            case ir.ApplyLocal(name=name, function=function, args=args):
                fields = {"name": name, "function": self.node(function), "args": self.nodes(args)}
            case ir.Prelude(op=op, args=args):
                fields = {"op": op, "args": self.nodes(args)}
            case ir.CoreCall(function=function, args=args):
                fields = {"function": self.function(function), "args": self.nodes(args)}
            case ir.Call(capability=capability, argument=argument):
                fields = {"capability": self.capability(capability), "argument": self.node(argument)}
            case ir.Get(record=record, field=field):
                fields = {"record": self.node(record), "field": field}
            case ir.MakeRecord(fields=parts):
                fields = {"fields": [[name, self.node(part)] for name, part in parts]}
            case ir.MakeList(items=items, element=element):
                fields = {"items": self.nodes(items), "element": self.type(element)}
            case ir.MakeTuple(items=items):
                fields = {"items": self.nodes(items)}
            case ir.MakeBracket(items=items, shape=shape):
                fields = {"items": self.nodes(items), "shape": self.type(shape)}
            case ir.MakeEmptyBrace(shape=shape):
                fields = {"shape": self.type(shape)}
            case ir.Wrap(kind=kind, value=value, type=t):
                fields = {"kind": kind, "value": self.node(value), "type": self.type(t)}
            case ir.MakeVariant(enum=enum, variant=variant, payload=payload):
                fields = {"enum": enum, "variant": variant, "payload": None if payload is None else self.node(payload)}
            case ir.Coerce(value=value, conversion=conversion):
                fields = {"value": self.node(value), "conversion": self.conversion(conversion)}
            case ir.Attempt(body=body):
                fields = {"body": self.node(body)}
            case ir.Retry(delays=delays, kinds=kinds, body=body):
                fields = {"delays": plain(delays), "kinds": plain(sorted(kinds)), "body": self.node(body)}
            case ir.WaitUntil(intervals=intervals, predicate=predicate, body=body):
                fields = {"intervals": plain(intervals), "predicate": self.node(predicate), "body": self.node(body)}
            case ir.Loop(maximum=maximum, binders=binders, initial=initial, body=body):
                fields = {
                    "maximum": maximum,
                    "binders": [self.binder(b) for b in binders],
                    "initial": self.nodes(initial),
                    "body": self.node(body),
                }
            case ir.Recur(args=args):
                fields = {"args": self.nodes(args)}
            case ir.Par(branches=branches):
                fields = {"branches": self.nodes(branches)}
            case ir.ParMap(limit=limit, function=function, items=items):
                fields = {"limit": limit, "function": self.node(function), "items": self.node(items)}
        return {"node": type(node).__name__, **fields, "span": span(node.span)}


def plain(items: Iterable[Json]) -> list[Json]:
    return list(items)


def span(s: Span) -> Json:
    return [s.start.line, s.start.column, s.end.line, s.end.column]


def export_program(program: ir.Program, inputs: Mapping[str, Type]) -> dict[str, Json]:
    exporter = _Exporter()
    body = exporter.node(program.body)
    result = exporter.type(program.result)
    declared = {name: exporter.type(inputs[name]) for name in sorted(program.inputs)}
    return {
        "body": body,
        "result": result,
        "capabilities": plain(sorted(program.capabilities)),
        "inputs": declared,
        "enums": dict(sorted(exporter.enums.items())),
        "signatures": {
            "capabilities": dict(sorted(exporter.capabilities.items())),
            "functions": dict(sorted(exporter.functions.items())),
        },
    }
