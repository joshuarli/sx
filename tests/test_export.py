import json
import typing
from dataclasses import fields
from pathlib import Path

import pytest

from sx import ir
from sx.cli import main
from sx.export import _Exporter, export_program
from sx.program import compile_source
from sx.registry import Capability, Effect, Function, Registry
from sx.syntax import Position, Span
from sx.types import INT, STRING, ListType, ResultType

SPAN = Span(Position(1, 1), Position(1, 2))
ONE = ir.Literal(1, INT, SPAN)
LAMBDA = ir.Lambda((ir.BindName("x"),), (INT,), INT, ONE, None, SPAN)
CAPABILITY = Capability("store.get", Effect.READ, INT, INT, "Reads.")
FUNCTION = Function("text.len", (STRING,), INT, "Length.")

SAMPLES: dict[type, typing.Any] = {
    ir.BindName: ir.BindName("x"),
    ir.Discard: ir.Discard(),
    ir.BindTuple: ir.BindTuple((ir.BindName("a"), ir.Discard())),
    ir.MatchSome: ir.MatchSome(ir.MatchAny(ir.Discard())),
    ir.MatchNone: ir.MatchNone(),
    ir.MatchOk: ir.MatchOk(ir.MatchAny(ir.Discard())),
    ir.MatchErr: ir.MatchErr(ir.MatchAny(ir.Discard())),
    ir.MatchVariant: ir.MatchVariant("OnBranch", ir.MatchAny(ir.BindName("info"))),
    ir.MatchLiteral: ir.MatchLiteral(3),
    ir.MatchAny: ir.MatchAny(ir.BindName("v")),
    ir.ToSome: ir.ToSome(None),
    ir.EachField: ir.EachField((("a", ir.VariantName()),)),
    ir.WidenRecord: ir.WidenRecord((ir.WidenedField("a", True, None), ir.WidenedField("b", False, None))),
    ir.EachItem: ir.EachItem((None, ir.ToSome(None))),
    ir.EachElement: ir.EachElement(ir.ToSome(None)),
    ir.VariantName: ir.VariantName(),
    ir.Literal: ONE,
    ir.Variable: ir.Variable("x", SPAN),
    ir.Let: ir.Let(ir.BindName("x"), ONE, ONE, SPAN),
    ir.LocalFunction: ir.LocalFunction("f", ONE, SPAN),
    ir.If: ir.If(ONE, ONE, ONE, SPAN),
    ir.Match: ir.Match(ONE, ((ir.MatchAny(ir.Discard()), ONE),), SPAN),
    ir.Lambda: LAMBDA,
    ir.ApplyLocal: ir.ApplyLocal("f", LAMBDA, (ONE,), SPAN),
    ir.Prelude: ir.Prelude("+", (ONE, ONE), SPAN),
    ir.CoreCall: ir.CoreCall(FUNCTION, (ONE,), SPAN),
    ir.Call: ir.Call(CAPABILITY, ONE, SPAN),
    ir.Get: ir.Get(ONE, "a", SPAN),
    ir.MakeRecord: ir.MakeRecord((("a", ONE),), SPAN),
    ir.MakeList: ir.MakeList((ONE,), INT, SPAN),
    ir.MakeTuple: ir.MakeTuple((ONE,), SPAN),
    ir.MakeBracket: ir.MakeBracket((ONE,), ListType(INT), SPAN),
    ir.MakeEmptyBrace: ir.MakeEmptyBrace(ListType(INT), SPAN),
    ir.Wrap: ir.Wrap("ok", ONE, ResultType(INT, STRING), SPAN),
    ir.MakeVariant: ir.MakeVariant("git.Checkout", "OnBranch", ONE, SPAN),
    ir.Coerce: ir.Coerce(ONE, ir.ToSome(None), SPAN),
    ir.Attempt: ir.Attempt(ONE, SPAN),
    ir.Retry: ir.Retry((100,), frozenset({"Timeout"}), ONE, SPAN),
    ir.WaitUntil: ir.WaitUntil((5,), LAMBDA, ONE, SPAN),
    ir.Sleep: ir.Sleep(5, SPAN),
    ir.Loop: ir.Loop(3, (ir.BindName("x"),), (ONE,), ONE, SPAN),
    ir.Recur: ir.Recur((ONE,), SPAN),
    ir.Par: ir.Par((ONE,), SPAN),
    ir.ParMap: ir.ParMap(2, LAMBDA, ONE, SPAN),
}

UNIONS = {
    "binder": ir.Binder,
    "pattern": ir.MatchPattern,
    "conversion": ir.Conversion,
    "node": ir.Node,
}


def members(alias: typing.TypeAliasType) -> set[type]:
    return set(typing.get_args(alias.__value__))


def test_samples_cover_every_ir_class() -> None:
    assert set(SAMPLES) == set().union(*(members(alias) for alias in UNIONS.values()))


def export(sample: object) -> typing.Any:
    kind = next(name for name, alias in UNIONS.items() if type(sample) in members(alias))
    return getattr(_Exporter(), kind)(sample)


@pytest.mark.parametrize("cls", list(SAMPLES), ids=lambda cls: cls.__name__)
def test_export_writes_every_field(cls: type) -> None:
    sample = SAMPLES[cls]
    exported = export(sample)
    names = {f.name for f in fields(sample)}
    if cls in members(ir.Node):
        assert isinstance(exported, dict)
        assert exported["node"] == cls.__name__
        assert names <= set(exported)
    elif names:
        assert isinstance(exported, dict)
        assert len(exported) == len(names)
    else:
        assert isinstance(exported, str)


@pytest.mark.parametrize("name", ["binder", "pattern", "conversion"])
def test_export_tags_tell_classes_apart(name: str) -> None:
    tags = []
    for cls in members(UNIONS[name]):
        exported = export(SAMPLES[cls])
        tags.append(exported if isinstance(exported, str) else next(iter(exported)))
    assert len(set(tags)) == len(tags)


def test_export_resolves_inferred_types_and_records_signatures() -> None:
    registry = Registry()
    registry.add_capability(Capability("store.get", Effect.READ, ListType(INT), INT, "Reads."))
    source = "[(some (call store.get [])) none]"
    checked = compile_source(source, registry, allowed=frozenset({"store.get"}))
    assert checked.program is not None
    exported: typing.Any = json.loads(json.dumps(export_program(checked.program, {})))
    none_literal = exported["body"]["items"][1]
    assert none_literal == {
        "node": "Literal", "value": "none", "type": {"option": "Int"}, "span": [1, 29, 1, 33],
    }
    assert exported["body"]["element"] == {"option": "Int"}
    assert exported["signatures"]["capabilities"] == {
        "store.get": {"effect": "read", "idempotent": False, "input": {"list": "Int"}, "output": "Int"},
    }
    assert exported["capabilities"] == ["store.get"]


def test_export_annotates_lambda_parameters(tmp_path: Path, capsys: pytest.CaptureFixture[str]) -> None:
    program = tmp_path / "p.sx"
    program.write_text("(map (fn [x] (some x)) [1 2])")
    assert main(["check", str(program), "--emit", "ir"]) == 0
    body = json.loads(capsys.readouterr().out)["body"]
    function = body["args"][0]
    assert (function["param_types"], function["result"]) == (["Int"], {"option": "Int"})
