from __future__ import annotations

import random
import struct
from collections.abc import Callable
from dataclasses import dataclass

from sx.registry import Registry
from sx.types import (
    CALL_ERROR_KIND,
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

from .participants import FROZEN_KERNEL, JSON_CORE


@dataclass(frozen=True, slots=True)
class Ty:
    head: str
    args: tuple[Ty, ...] = ()
    fields: tuple[tuple[str, Ty], ...] = ()
    name: str = ""


INT = Ty("Int")
FLOAT = Ty("Float")
STRING = Ty("String")
BOOL = Ty("Bool")
UNIT = Ty("Unit")
BYTES = Ty("Bytes")
LEVEL = Ty("Enum", name="Level")
KIND = Ty("Enum", name="CallErrorKind")
SHAPE = Ty("Enum", name="Shape")
CALL_ERROR = Ty("Record", fields=(("kind", KIND), ("message", STRING)))


def lst(t: Ty) -> Ty:
    return Ty("List", (t,))


def opt(t: Ty) -> Ty:
    return Ty("Option", (t,))


def res(ok: Ty, err: Ty) -> Ty:
    return Ty("Result", (ok, err))


def tup(*items: Ty) -> Ty:
    return Ty("Tuple", items)


def mp(value: Ty) -> Ty:
    return Ty("Map", (value,))


def rec(*fields: tuple[str, Ty]) -> Ty:
    return Ty("Record", fields=fields)


def to_ty(t: Type, enums: dict[str, tuple[tuple[str, Ty | None], ...]]) -> Ty:
    match t:
        case Primitive(name=name):
            return Ty(name)
        case ListType(element=element):
            return lst(to_ty(element, enums))
        case MapType(value=value):
            return mp(to_ty(value, enums))
        case TupleType(items=items):
            return Ty("Tuple", tuple(to_ty(item, enums) for item in items))
        case OptionType(inner=inner):
            return opt(to_ty(inner, enums))
        case ResultType(ok=ok, err=err):
            return res(to_ty(ok, enums), to_ty(err, enums))
        case RecordType(fields=fields):
            return Ty("Record", fields=tuple((name, to_ty(ft, enums)) for name, ft in fields))
        case EnumType(name=name, variants=variants):
            if name not in enums:
                enums[name] = ()
                enums[name] = tuple((v, None if p is None else to_ty(p, enums)) for v, p in variants)
            return Ty("Enum", name=name)
        case NominalType(base=base):
            return to_ty(base, enums)
    raise AssertionError(t)


@dataclass(frozen=True, slots=True)
class Cap:
    id: str
    input: Ty
    output: Ty
    write: bool
    idempotent: bool


@dataclass(frozen=True, slots=True)
class Binding:
    name: str
    ty: Ty | None
    params: tuple[Ty, ...] = ()
    result: Ty | None = None


@dataclass(frozen=True, slots=True)
class Ctx:
    env: tuple[Binding, ...] = ()
    depth: int = 6
    writes: str = "any"
    loop: tuple[Binding, ...] | None = None

    def down(self, by: int = 1) -> Ctx:
        return Ctx(self.env, self.depth - by, self.writes, self.loop)

    def with_env(self, *bindings: Binding) -> Ctx:
        return Ctx((*self.env, *bindings), self.depth, self.writes, self.loop)

    def mode(self, writes: str) -> Ctx:
        return Ctx(self.env, self.depth, writes, self.loop)

    def visible(self) -> list[Binding]:
        latest: dict[str, Binding] = {}
        for binding in self.env:
            latest[binding.name] = binding
        return list(latest.values())

    def names(self) -> set[str]:
        return {binding.name for binding in self.env}


type Body = Callable[[Ctx, int], str]
type Clause = tuple[str, list[Binding]]

PRELUDE_NAMES = (
    "count", "first", "rest", "nth", "take", "concat", "flatten", "keys", "vals", "entries", "identity", "map",
    "filter", "keep", "reduce", "str", "float", "round", "floor", "ceil", "quot", "rem", "not", "and", "or",
    "contains?", "assoc", "dissoc", "any?", "all?", "sort-by", "group-by", "distinct-by", "into-map",
)
WORDS = ("a", "b", "c", "d", "ab", "x", "key", "name", "id", "zed", "", "A", "é", "é", "😀", "￿", "a b")
INT_EDGES = (0, 1, -1, 2, 3, 5, 10, 100, 2**31, 2**53, 2**53 + 1, 2**63 - 1, -(2**63), 2**62, -(2**62), 3037000500)
FLOAT_EDGES = (
    "0.0", "-0.0", "1.0", "-1.0", "0.5", "1.5", "2.5", "-2.5", "0.1", "0.2", "0.3", "1e21", "1e20", "1e-6", "1e-7",
    "1e300", "1.7976931348623157e308", "5e-324", "123456789012345680000.0", "2e3", "1.5E-7", "9007199254740993.0",
    "9223372036854775807.0", "9223372036854775808.0", "-9223372036854775808.0", "4.9e-324", "100.0", "3.0",
)
REFUSAL = ("RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict")
ALL_KINDS = (*REFUSAL, "Timeout", "Unavailable", "Failed")
DURATIONS = ("1ms", "250ms", "1s", "2s", "5s", "30s", "2m", "1h")


JSON_TEXTS = (
    '{"a": 1, "b": "x", "c": [true, null]}',
    '{"a": {"b": "x"}, "c": []}',
    "[1, 2.50, \"s\"]",
    '"text"',
    "null",
    "{",
    "[1,]",
    '{"a": }',
    '{"a": 1, "a": 2}',
    "nope",
    "",
)
JSON_PATHS = ([], ["a"], ["a", "b"], ["c", "0"], ["0"], ["1"], ["zz"])


def quote(text: str) -> str:
    out = ['"']
    for ch in text:
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
        elif ord(ch) < 32 or ord(ch) == 0xFFFF:
            out.append(f"\\u{{{ord(ch):X}}}")
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


class Gen:
    def __init__(self, rng: random.Random, registry: Registry, concurrency: int = 16) -> None:
        self.rng = rng
        self.concurrency = concurrency
        self.features: set[str] = {FROZEN_KERNEL}
        self.counter = 0
        self.finals = 0
        self.enums: dict[str, tuple[tuple[str, Ty | None], ...]] = {}
        self.enums["CallErrorKind"] = tuple((v, None) for v, _ in CALL_ERROR_KIND.variants)
        self.caps = [
            Cap(
                c.id,
                to_ty(c.input, self.enums),
                to_ty(c.output, self.enums),
                c.effect.value == "write",
                c.idempotent,
            )
            for c in registry.capabilities.values()
        ]
        self.records = [to_ty(t, self.enums) for t in registry.types.values() if isinstance(t, RecordType)]
        self.records = list(dict.fromkeys(self.records))
        for t in registry.types.values():
            if isinstance(t, EnumType):
                to_ty(t, self.enums)
        self.path_cache: dict[tuple[Ty, Ty], list[tuple[tuple[str, str | int], ...]]] = {}

    def chance(self, p: float) -> bool:
        return self.rng.random() < p

    def pick[T](self, items: list[T] | tuple[T, ...]) -> T:
        return self.rng.choice(items)

    def weighted(self, options: list[tuple[float, Callable[[], str]]]) -> str:
        weights = [w for w, _ in options]
        return self.rng.choices([f for _, f in options], weights=weights)[0]()

    def fresh(self, base: str = "v") -> str:
        self.counter += 1
        return f"{base}{self.counter}"

    def name(self, ctx: Ctx) -> str:
        roll = self.rng.random()
        if roll < 0.05:
            return self.pick(PRELUDE_NAMES)
        if roll < 0.12:
            return self.pick(["x", "item", "acc", "n", "r", "s", "total"])
        return self.fresh(self.pick(["v", "x", "item", "n", "acc", "t", "r", "s"]))

    def hidden(self, ctx: Ctx, prelude: str) -> bool:
        return prelude in ctx.names()

    def rand_ty(self, depth: int = 2, sortable: bool = False, plain: bool = False) -> Ty:
        weights: list[tuple[float, Callable[[], Ty]]] = [
            (6, lambda: INT),
            (5, lambda: STRING),
            (2, lambda: BOOL),
            (2, lambda: FLOAT),
        ]
        if not sortable:
            weights.append((1, lambda: UNIT))
        if not plain and not sortable:
            weights.append((2, lambda: self.pick(self.records)))
            weights.append((1, lambda: LEVEL))
        if depth > 0:
            weights += [
                (4, lambda: lst(self.rand_ty(depth - 1, sortable, plain))),
                (3, lambda: opt(self.encodable_inner(depth - 1))),
                (3, lambda: tup(*[self.rand_ty(depth - 1, sortable, plain) for _ in range(self.rng.randint(2, 3))])),
            ]
            if not sortable:
                weights += [
                    (2, lambda: mp(self.map_value(depth - 1))),
                    (1, lambda: res(self.rand_ty(depth - 1), self.pick([STRING, CALL_ERROR, INT]))),
                    (
                        1,
                        lambda: rec(
                            *[
                                (self.pick(["a", "b", "c", "d"]) + str(i), self.rand_ty(depth - 1))
                                for i in range(self.rng.randint(1, 3))
                            ]
                        ),
                    ),
                ]
        return self.rng.choices([f for _, f in weights], weights=[w for w, _ in weights])[0]()

    def encodable_inner(self, depth: int) -> Ty:
        for _ in range(10):
            t = self.rand_ty(depth)
            if t.head not in {"Option", "Unit"}:
                return t
        return INT

    def map_value(self, depth: int) -> Ty:
        for _ in range(10):
            t = self.rand_ty(depth)
            if t.head != "Option":
                return t
        return INT

    def paths(self, source: Ty, target: Ty, limit: int = 2) -> list[tuple[tuple[str, str | int], ...]]:
        key = (source, target)
        if key in self.path_cache and limit == 2:
            return self.path_cache[key]
        found: list[tuple[tuple[str, str | int], ...]] = []
        if source == target:
            found.append(())
        if limit > 0:
            if source.head == "Record":
                for name, ft in source.fields:
                    found.extend((("get", name), *tail) for tail in self.paths(ft, target, limit - 1))
                    if ft.head == "Option":
                        found.extend((("getd", name), *tail) for tail in self.paths(ft.args[0], target, limit - 1))
            elif source.head == "List":
                for tail in self.paths(source.args[0], target, limit - 1):
                    found.append((("first", 0), *tail))
                    found.append((("nth", 1), *tail))
            elif source.head == "Tuple":
                for i, item in enumerate(source.args):
                    found.extend((("tnth", i), *tail) for tail in self.paths(item, target, limit - 1))
            elif source.head == "Map":
                found.extend((("mget", 0), *tail) for tail in self.paths(source.args[0], target, limit - 1))
            if target == INT and source.head in {"List", "Map"}:
                found.append((("count", 0),))
            if target == INT and source == BYTES:
                found.append((("count", 0),))
            if target == STRING and source in {INT, FLOAT, BOOL, LEVEL, KIND}:
                found.append((("str", 0),))
        if limit == 2:
            self.path_cache[key] = found
        return found

    def render_path(self, src: str, steps: tuple[tuple[str, str | int], ...], ctx: Ctx, source: Ty) -> str:
        current = source
        for kind, arg in steps:
            if kind == "get":
                src = f"(get {src} :{arg})"
                current = dict(current.fields)[str(arg)]
            elif kind == "getd":
                inner = dict(current.fields)[str(arg)].args[0]
                src = f"(get {src} :{arg} {self.leaf(inner, ctx, True)})"
                current = inner
            elif kind == "first":
                src = f"(first {src})"
                current = current.args[0]
            elif kind == "nth":
                src = f"(nth {src} {self.rng.randint(0, 2)})"
                current = current.args[0]
            elif kind == "tnth":
                assert isinstance(arg, int)
                src = f"(nth {src} {arg})"
                current = current.args[arg]
            elif kind == "mget":
                src = f"(get {src} {quote(self.pick(WORDS))} {self.leaf(current.args[0], ctx, True)})"
                current = current.args[0]
            elif kind == "count":
                src = f"(count {src})"
                current = INT
            elif kind == "str":
                src = f"(str {src})"
                current = STRING
        return src

    def env_derivations(self, ty: Ty, ctx: Ctx) -> list[str]:
        out: list[str] = []
        for binding in ctx.visible():
            if binding.ty is None:
                continue
            out.extend(self.render_path(binding.name, steps, ctx, binding.ty) for steps in self.paths(binding.ty, ty))
        return out

    def cap_available(self, cap: Cap, ctx: Ctx) -> bool:
        if not cap.write:
            return True
        if ctx.writes == "none":
            return False
        if ctx.writes == "idempotent":
            return cap.idempotent
        return True

    def cap_call(self, cap: Cap, ctx: Ctx) -> str:
        return f"(call {cap.id} {self.expr(cap.input, ctx.down(2), known=True)})"

    def cap_options(self, ty: Ty, ctx: Ctx) -> list[tuple[float, Callable[[], str]]]:
        options: list[tuple[float, Callable[[], str]]] = []
        for cap in self.caps:
            if not self.cap_available(cap, ctx):
                continue
            options.extend(
                (
                    1.0 if steps else 2.5,
                    lambda cap=cap, steps=steps: self.render_path(self.cap_call(cap, ctx), steps, ctx, cap.output),
                )
                for steps in self.paths(cap.output, ty)
            )
        return options

    def leaf(self, ty: Ty, ctx: Ctx, known: bool = False, loose: int = 0) -> str:
        env = [b for b in ctx.visible() if b.ty == ty]
        if env and self.chance(0.5):
            return self.pick(env).name
        match ty.head:
            case "Int":
                return self.int_literal()
            case "Float":
                return self.float_literal()
            case "String":
                return quote(self.pick(WORDS))
            case "Bool":
                return self.pick(["true", "false"])
            case "Unit":
                return "(sleep 0ms)"
            case "List":
                return "[]" if self.chance(0.4) else f"[{self.leaf(ty.args[0], ctx, known)}]"
            case "Option":
                if self.chance(0.5):
                    return "none"
                return f"(some {self.leaf(ty.args[0], ctx, known)})" if not loose or self.chance(0.5) else self.leaf(
                    ty.args[0], ctx, known
                )
            case "Tuple":
                return "[" + " ".join(self.leaf(t, ctx, known) for t in ty.args) + "]"
            case "Map":
                return f"(assoc {{}} {quote(self.pick(WORDS))} {self.leaf(ty.args[0], ctx, known)})"
            case "Record":
                return self.record_literal(ty, ctx, known, loose, leaf=True)
            case "Result":
                if ty.args[1] == CALL_ERROR:
                    return f"(attempt {self.leaf(ty.args[0], ctx, known)})"
                if self.chance(0.5):
                    return f"(ok {self.leaf(ty.args[0], ctx, known)})"
                return f"(err {self.leaf(ty.args[1], ctx, known)})"
            case "Enum":
                variants = self.enums[ty.name]
                plain = [v for v, p in variants if p is None]
                if known and plain:
                    variant = self.pick(plain)
                    return variant if self.chance(0.5) else quote(variant)
                options = self.cap_options(ty, ctx.mode("none"))
                if options:
                    return self.weighted(options)
                return self.pick(plain)
            case "Bytes":
                return '(call blob.get {:name "a"})'
        raise AssertionError(ty)

    def int_literal(self) -> str:
        if self.chance(0.7):
            return str(self.rng.randint(-3, 12))
        edge = self.pick(INT_EDGES)
        return str(-edge if self.chance(0.3) and edge != -(2**63) else edge)

    def float_literal(self) -> str:
        if self.chance(0.4):
            text = f"{self.rng.randint(-40, 40) / self.pick([1, 2, 4, 10])!r}"
            return text if "." in text or "e" in text else text + ".0"
        text = self.pick(FLOAT_EDGES)
        return text if self.chance(0.8) or text.startswith("-") else "-" + text

    def record_literal(self, ty: Ty, ctx: Ctx, known: bool, loose: int, leaf: bool = False) -> str:
        parts = []
        for fname, fty in ty.fields:
            if fty.head == "Option" and (known or loose == 2) and self.chance(0.35 if loose != 1 else 0.0):
                continue
            value = self.leaf(fty, ctx, known, loose) if leaf else self.expr(fty, ctx.down(), known, loose)
            parts.append(f":{fname} {value}")
        if loose == 0 and not known and self.chance(0.3):
            self.rng.shuffle(parts)
        return "{" + " ".join(parts) + "}"

    def expr(self, ty: Ty, ctx: Ctx, known: bool = False, loose: int = 0) -> str:
        if ctx.depth <= 0:
            return self.leaf(ty, ctx, known, loose)
        c = ctx.down()
        options: list[tuple[float, Callable[[], str]]] = [(2.0, lambda: self.leaf(ty, c, known, loose))]
        derived = self.env_derivations(ty, c)
        if derived:
            options.append((7.0, lambda: self.pick(derived)))
        options.extend(self.cap_options(ty, c))

        def sub(cc: Ctx, i: int) -> str:
            return self.expr(ty, cc, known=False, loose=self.branch_looseness(loose, i))

        options += [
            (1.8, lambda: self.let_form(ty, c, sub)),
            (1.5, lambda: self.if_form(ty, c, sub)),
            (0.7, lambda: self.cond_form(ty, c, sub)),
            (1.8, lambda: self.match_form(ty, c, sub)),
            (0.9, lambda: self.retry_form(ty, c)),
            (0.5, lambda: self.wait_form(ty, c)),
            (0.8, lambda: self.loop_form(ty, c)),
            (0.8, lambda: self.reduce_form(ty, c)),
            (0.6, lambda: f"(first {self.expr(lst(ty), c)})"),
            (4.0, lambda: self.fn_apply(ty, c)),
        ]
        fns = [b for b in c.visible() if b.ty is None and b.result == ty]
        if not fns:
            options.pop()
        options += self.specific(ty, c, known, loose)
        if self.joinable(ty):
            options.append((4.0, lambda: self.join_template(ty, c)))
        if loose or known:
            options += self.loose_options(ty, c, known)
        return self.weighted(options)

    def joinable(self, ty: Ty) -> bool:
        if ty == STRING or ty.head == "Option":
            return True
        if ty.head == "List":
            return self.joinable(ty.args[0])
        if ty.head == "Tuple":
            return any(self.joinable(t) for t in ty.args)
        if ty.head == "Record":
            return any(self.joinable(t) for _, t in ty.fields)
        return False

    def view(self, ty: Ty) -> Ty | None:
        if ty == STRING:
            return self.pick([LEVEL, KIND])
        if ty.head == "Option":
            inner = self.view(ty.args[0]) if self.chance(0.4) else None
            return opt(inner) if inner is not None else ty.args[0]
        if ty.head == "List":
            element = self.view(ty.args[0])
            return lst(element) if element is not None else None
        if ty.head == "Tuple":
            indexes = [i for i, t in enumerate(ty.args) if self.view(t) is not None]
            if not indexes:
                return None
            at = self.pick(indexes)
            changed = self.view(ty.args[at])
            assert changed is not None
            return tup(*[changed if i == at else t for i, t in enumerate(ty.args)])
        if ty.head == "Record":
            options: list[tuple[str, Ty | None]] = []
            for name, field_ty in ty.fields:
                if field_ty.head == "Option":
                    options.append((name, None))
                altered = self.view(field_ty)
                if altered is not None:
                    options.append((name, altered))
            if not options:
                return None
            name, altered = self.pick(options)
            if altered is None:
                return Ty("Record", fields=tuple(f for f in ty.fields if f[0] != name))
            return Ty("Record", fields=tuple((n, altered if n == name else t) for n, t in ty.fields))
        return None

    def join_template(self, ty: Ty, ctx: Ctx) -> str:
        altered = self.view(ty)
        if altered is None:
            return self.expr(ty, ctx)
        first, second = self.expr(ty, ctx.down()), self.expr(altered, ctx.down())
        if self.chance(0.6):
            name = self.fresh("w")
            second = f"(let [{name} {second}] {name})"
        if self.chance(0.15):
            first, second = second, first
        if self.chance(0.5):
            test, _ = self.cond_test(ctx)
            return f"(if {test} {first} {second})"
        if self.chance(0.5):
            return f"(cond {self.expr(BOOL, ctx.down())} {first} :else {second})"
        return f"(match {self.expr(BOOL, ctx.down())} true {first} false {second})"

    def branch_looseness(self, loose: int, index: int) -> int:
        if loose:
            return min(loose, 1) if index == 0 else loose
        if self.chance(0.5):
            return 1 if index == 0 else 2
        return 0

    def loose_options(self, ty: Ty, ctx: Ctx, known: bool) -> list[tuple[float, Callable[[], str]]]:
        if ty.head == "Option":
            return [(6.0, lambda: self.expr(ty.args[0], ctx, known, 1))]
        if ty == STRING:
            return [(6.0, lambda: self.expr(self.pick([LEVEL, KIND]), ctx, False, 0))]
        return []

    def fn_apply(self, ty: Ty, ctx: Ctx) -> str:
        fns = [b for b in ctx.visible() if b.ty is None and b.result == ty]
        fn = self.pick(fns)
        args = " ".join(self.expr(p, ctx.down()) for p in fn.params)
        return f"({fn.name} {args})"

    def json_call(self, function: str) -> str:
        self.features.add(JSON_CORE)
        text = quote(self.pick(JSON_TEXTS))
        path = "[" + " ".join(quote(part) for part in self.pick(JSON_PATHS)) + "]"
        return f"({function} {text} {path})"

    def specific(self, ty: Ty, ctx: Ctx, known: bool, loose: int) -> list[tuple[float, Callable[[], str]]]:
        e = self.expr
        o: list[tuple[float, Callable[[], str]]] = []
        h = ty.head
        if h == "Int":
            o += [
                (3.0, lambda: f"({self.pick(['+', '-', '*'])} {e(INT, ctx)} {e(INT, ctx)})"),
                (0.6, lambda: f"(- {e(INT, ctx)})"),
                (1.5, lambda: f"({self.pick(['quot', 'rem'])} {e(INT, ctx)} {e(INT, ctx)})"),
                (1.5, lambda: f"({self.pick(['round', 'floor', 'ceil'])} {e(FLOAT, ctx)})"),
                (1.5, lambda: f"(count {e(self.pick_container(), ctx)})"),
                (0.8, lambda: f"(nth {e(lst(INT), ctx)} {e(INT, ctx)})"),
            ]
            o = [x for x in o if True]
        elif h == "Float":
            o += [
                (3.0, lambda: f"({self.pick(['+', '-', '*', '/'])} {e(FLOAT, ctx)} {e(FLOAT, ctx)})"),
                (0.6, lambda: f"(- {e(FLOAT, ctx)})"),
                (1.5, lambda: f"(float {e(INT, ctx)})"),
            ]
        elif h == "Bool":
            o += [
                (3.0, lambda: self.compare(ctx)),
                (1.0, lambda: f"({self.pick(['and', 'or'])} {e(BOOL, ctx)} {e(BOOL, ctx)})"),
                (0.7, lambda: f"(not {e(BOOL, ctx)})"),
                (1.0, lambda: self.contains(ctx)),
                (1.2, lambda: self.any_all(ctx)),
            ]
        elif h == "String":
            o += [
                (3.0, lambda: self.str_call(ctx)),
                (0.6, lambda: f"(first {e(lst(STRING), ctx)})"),
            ]
        elif h == "Unit":
            o += [(2.0, lambda: f"(sleep {self.pick((*DURATIONS, '0ms'))})")]
        elif h == "List":
            o += self.list_options(ty, ctx, known, loose)
        elif h == "Option":
            inner = ty.args[0]
            o += [
                (3.0, lambda: f"(some {e(inner, ctx, known, loose)})"),
                (2.0, lambda: "none"),
                (1.5, lambda: f"(get {e(mp(inner), ctx)} {e(STRING, ctx)})"),
                (1.0, lambda: f"(first {e(lst(ty), ctx)})"),
            ]
            if inner == STRING:
                o.append((6.0, lambda: self.json_call("json.get")))
        elif h == "Result":
            ok, err = ty.args
            if err == CALL_ERROR:
                o += [(4.0, lambda: f"(attempt {e(ok, ctx)})")]
            o += [
                (2.0, lambda: f"(ok {e(ok, ctx)})"),
                (2.0, lambda: f"(err {e(err, ctx)})"),
            ]
        elif h == "Tuple":
            o += [
                (4.0, lambda: "[" + " ".join(e(t, ctx, known, loose) for t in ty.args) + "]"),
                (1.5, lambda: self.par_form(ty, ctx)),
            ]
        elif h == "Map":
            v = ty.args[0]
            o += [
                (3.0, lambda: f"(assoc {e(ty, ctx)} {e(STRING, ctx)} {e(v, ctx)})"),
                (1.5, lambda: f"(assoc {{}} {e(STRING, ctx)} {e(v, ctx)})"),
                (1.0, lambda: f"(dissoc {e(ty, ctx)} {e(STRING, ctx)})"),
                (2.0, lambda: f"(into-map {e(lst(tup(STRING, v)), ctx)})"),
            ]
        elif h == "Record":
            o += [(4.0, lambda: self.record_literal(ty, ctx, known, loose))]
        elif h == "Enum":
            o += []
        return [(w, f) for w, f in o if self.prelude_ok(f, ctx)]

    def prelude_ok(self, _f: Callable[[], str], _ctx: Ctx) -> bool:
        return True

    def pick_container(self) -> Ty:
        return self.pick([lst(INT), lst(STRING), mp(INT), BYTES, lst(self.pick(self.records)), lst(lst(INT))])

    def compare(self, ctx: Ctx) -> str:
        e = self.expr
        if self.chance(0.5):
            u = self.pick([INT, FLOAT, STRING])
            return f"({self.pick(['<', '<=', '>', '>='])} {e(u, ctx)} {e(u, ctx)})"
        u = self.rand_ty(1)
        if u.head == "Unit":
            u = INT
        if u.head == "Enum" or (self.chance(0.2) and u.head == "Record"):
            u = LEVEL if self.chance(0.7) else KIND
        return f"({self.pick(['=', 'not='])} {e(u, ctx)} {e(u, ctx, known=True)})"

    def contains(self, ctx: Ctx) -> str:
        e = self.expr
        if self.chance(0.4):
            return f"(contains? {e(mp(self.map_value(1)), ctx)} {e(STRING, ctx)})"
        u = self.pick([INT, STRING, self.rand_ty(1, plain=True)])
        if u.head == "Unit":
            u = INT
        return f"(contains? {e(lst(u), ctx)} {e(u, ctx)})"

    def any_all(self, ctx: Ctx) -> str:
        u = self.rand_ty(1)
        return f"({self.pick(['any?', 'all?'])} {self.fn_arg([u], BOOL, ctx)} {self.expr(lst(u), ctx)})"

    def str_call(self, ctx: Ctx) -> str:
        n = self.rng.randint(1, 3)
        pool = [STRING, STRING, INT, FLOAT, BOOL, LEVEL, KIND]
        return "(str " + " ".join(self.expr(self.pick(pool), ctx) for _ in range(n)) + ")"

    def sort_key_ty(self) -> Ty:
        return self.pick([INT, STRING, FLOAT, BOOL, INT, STRING, tup(INT, STRING), lst(INT), tup(BOOL, FLOAT)])

    def key_fn(self, source: Ty, ctx: Ctx) -> str:
        key = self.sort_key_ty()
        return self.fn_arg([source], key, ctx, sort_key=True)

    def list_options(self, ty: Ty, ctx: Ctx, known: bool, loose: int) -> list[tuple[float, Callable[[], str]]]:
        e = self.expr
        t = ty.args[0]
        o: list[tuple[float, Callable[[], str]]] = [
            (3.0, lambda: "[" + " ".join(e(t, ctx, known, loose) for _ in range(self.rng.randint(0, 4))) + "]"),
            (3.0, lambda: self.map_form(t, ctx)),
            (2.0, lambda: f"(filter {self.fn_arg([t], BOOL, ctx)} {e(ty, ctx)})"),
            (1.5, lambda: self.keep_form(t, ctx)),
            (1.5, lambda: f"(sort-by {self.key_fn(t, ctx)} {e(ty, ctx)})"),
            (1.0, lambda: f"(distinct-by {self.key_fn(t, ctx)} {e(ty, ctx)})"),
            (1.0, lambda: f"(take {e(INT, ctx)} {e(ty, ctx)})"),
            (1.0, lambda: f"(concat {e(ty, ctx)} {e(ty, ctx)})"),
            (1.0, lambda: f"(rest {e(ty, ctx)})"),
            (1.0, lambda: f"(flatten {e(lst(ty), ctx)})"),
            (1.5, lambda: self.par_map_form(t, ctx)),
            (0.7, lambda: f"(vals {e(mp(t), ctx)})"),
        ]
        if t == STRING:
            o.append((1.0, lambda: f"(keys {e(mp(self.map_value(1)), ctx)})"))
            o.append((5.0, lambda: self.json_call("json.keys")))
        if t.head == "Tuple" and len(t.args) == 2 and t.args[0] == STRING:
            o.append((1.0, lambda: f"(entries {e(mp(t.args[1]), ctx)})"))
            if t.args[1].head == "List":
                element = t.args[1].args[0]
                o.append((1.0, lambda: f"(group-by {self.fn_arg([element], STRING, ctx)} {e(t.args[1], ctx)})"))
        if t.head == "Tuple" and len(t.args) == 2 and t.args[1].head == "List":
            key = t.args[0]
            src = t.args[1].args[0]
            o.append((1.5, lambda: f"(group-by {self.fn_arg([src], key, ctx, sort_key=True)} {e(t.args[1], ctx)})"))
        return o

    def map_form(self, t: Ty, ctx: Ctx) -> str:
        u = self.rand_ty(1)
        return f"(map {self.fn_arg([u], t, ctx)} {self.expr(lst(u), ctx)})"

    def keep_form(self, t: Ty, ctx: Ctx) -> str:
        u = self.rand_ty(1)
        return f"(keep {self.fn_arg([u], opt(t), ctx)} {self.expr(lst(u), ctx)})"

    def reduce_form(self, ty: Ty, ctx: Ctx) -> str:
        u = self.rand_ty(1)
        return f"(reduce {self.fn_arg([ty, u], ty, ctx)} {self.expr(ty, ctx)} {self.expr(lst(u), ctx)})"

    def par_form(self, ty: Ty, ctx: Ctx) -> str:
        pc = ctx.mode("none")
        return "(par " + " ".join(self.expr(t, pc) for t in ty.args) + ")"

    def par_map_form(self, t: Ty, ctx: Ctx) -> str:
        pc = ctx.mode("none")
        u = self.rand_ty(1)
        limit = self.rng.randint(1, max(1, self.concurrency))
        if self.chance(0.05):
            limit = self.concurrency + 1
        return f"(par-map {{:limit {limit}}} {self.fn_arg([u], t, pc)} {self.expr(lst(u), pc)})"

    def binder(self, ty: Ty, ctx: Ctx, allow_discard: bool = True) -> tuple[str, list[Binding]]:
        if allow_discard and self.chance(0.08):
            return "_", []
        if ty.head == "Tuple" and self.chance(0.5):
            parts = []
            binds: list[Binding] = []
            for item in ty.args:
                src, bs = self.binder(item, ctx)
                parts.append(src)
                binds += bs
            return "[" + " ".join(parts) + "]", binds
        name = self.name(ctx)
        return name, [Binding(name, ty)]

    def fn_arg(self, params: list[Ty], ret: Ty, ctx: Ctx, sort_key: bool = False) -> str:
        options: list[tuple[float, Callable[[], str]]] = [(6.0, lambda: self.lambda_form(params, ret, ctx))]
        options.extend(
            (1.5, lambda name=name: name)
            for name in self.prelude_fns(params, ret, sort_key)
            if not self.hidden(ctx, name)
        )
        options.extend(
            (3.0, lambda b=b: b.name)
            for b in ctx.visible()
            if b.ty is None and b.params == tuple(params) and b.result == ret
        )
        return self.weighted(options)

    def prelude_fns(self, params: list[Ty], ret: Ty, sort_key: bool) -> list[str]:
        out: list[str] = []
        if len(params) == 1:
            p = params[0]
            if p == ret:
                out.append("identity")
                if p == INT:
                    out.append("-")
                if p == FLOAT:
                    out.append("-")
                if p == BOOL:
                    out.append("not")
                if p.head == "List":
                    out.append("rest")
            if ret == INT and (p.head in {"List", "Map"} or p == BYTES):
                out.append("count")
            if ret == FLOAT and p == INT:
                out.append("float")
            if ret == INT and p == FLOAT:
                out += ["round", "floor", "ceil"]
            if p.head == "List" and p.args[0] == ret:
                out.append("first")
            if p.head == "List" and p.args[0].head == "List" and p.args[0] == ret:
                out.append("flatten")
            if p.head == "Map":
                if ret == lst(STRING):
                    out.append("keys")
                if ret == lst(p.args[0]):
                    out.append("vals")
            if ret == STRING and p in {STRING, INT, FLOAT, BOOL, LEVEL, KIND} and (not sort_key or p == STRING):
                out.append("str")
        if len(params) == 2:
            a, b = params
            if a == b == ret and a in {INT, FLOAT}:
                out += ["+", "-", "*"]
                if a == FLOAT:
                    out.append("/")
                else:
                    out += ["quot", "rem"]
            if a == b and ret == BOOL:
                out += ["=", "not="]
                if a in {INT, FLOAT, STRING}:
                    out += ["<", "<=", ">", ">="]
            if a == b == ret == BOOL:
                out += ["and", "or"]
            if a.head == "List" and ret == a and a == b:
                out.append("concat")
        return out

    def lambda_form(self, params: list[Ty], ret: Ty, ctx: Ctx) -> str:
        names: list[str] = []
        binds: list[Binding] = []
        for p in params:
            src, bs = self.binder(p, ctx)
            names.append(src)
            binds += bs
        inner = ctx.with_env(*binds)
        inner = Ctx(inner.env, inner.depth - 1, inner.writes, None)
        body = self.expr(ret, inner, loose=0)
        return f"(fn [{' '.join(names)}] {body})"

    def let_form(self, ty: Ty, ctx: Ctx, body: Body) -> str:
        count = self.rng.randint(1, 3)
        parts: list[str] = []
        cur = ctx
        for _ in range(count):
            if self.chance(0.2):
                np = self.rng.randint(1, 2)
                params = [self.rand_ty(1, plain=True) for _ in range(np)]
                ret = ty if self.chance(0.5) else self.rand_ty(1)
                name = self.name(cur)
                fn = self.lambda_form(params, ret, cur)
                parts += [name, fn]
                cur = cur.with_env(Binding(name, None, tuple(params), ret))
                continue
            vt = self.rand_ty(2) if self.chance(0.8) else self.pick([UNIT, INT])
            value = self.expr(vt, cur.down(self.rng.randint(0, 1)))
            src, binds = self.binder(vt, cur)
            parts += [src, value]
            cur = cur.with_env(*binds)
        return f"(let [{' '.join(parts)}] {body(cur, 0)})"

    def cond_test(self, ctx: Ctx) -> tuple[str, list[Binding]]:
        roll = self.rng.random()
        options = [b for b in ctx.visible() if b.ty is not None and b.ty.head == "Option" and b.ty.args[0] != BOOL]
        if roll < 0.15 and options:
            b = self.pick(options)
            assert b.ty is not None
            return b.name, [Binding(b.name, b.ty.args[0])]
        if roll < 0.3:
            inner = self.pick([INT, STRING, self.pick(self.records)])
            return self.expr(opt(inner), ctx.down()), []
        return self.expr(BOOL, ctx.down()), []

    def if_form(self, ty: Ty, ctx: Ctx, body: Body) -> str:
        test, narrowed = self.cond_test(ctx)
        then = body(ctx.with_env(*narrowed), 0)
        otherwise = body(ctx, 1)
        return f"(if {test} {then} {otherwise})"

    def cond_form(self, ty: Ty, ctx: Ctx, body: Body) -> str:
        parts: list[str] = []
        for i in range(self.rng.randint(1, 3)):
            test, narrowed = self.cond_test(ctx)
            parts += [test, body(ctx.with_env(*narrowed), i)]
        parts += [":else", body(ctx, 9)]
        return "(cond " + " ".join(parts) + ")"

    def pattern(self, ty: Ty, ctx: Ctx, depth: int = 1) -> tuple[str, list[Binding], bool]:
        if ty.head == "Option" and depth > 0 and self.chance(0.3):
            src, bs, _ = self.pattern(ty.args[0], ctx, depth - 1)
            return f"(some {src})", bs, False
        if ty.head in {"Int", "String", "Bool"} and self.chance(0.3):
            return self.literal_pattern(ty), [], False
        if self.chance(0.4):
            return "_", [], True
        name = self.name(ctx)
        return name, [Binding(name, ty)], True

    def literal_pattern(self, ty: Ty) -> str:
        if ty == INT:
            return str(self.rng.randint(-2, 4))
        if ty == STRING:
            return quote(self.pick(WORDS[:6]))
        return self.pick(["true", "false"])

    def cover_pattern(self, ty: Ty, ctx: Ctx, depth: int = 1) -> tuple[str, list[Binding]]:
        for _ in range(8):
            src, bs, covers = self.pattern(ty, ctx, depth)
            if covers:
                return src, bs
        name = self.name(ctx)
        return name, [Binding(name, ty)]

    def match_form(self, ty: Ty, ctx: Ctx, body: Body, scrutinee_ty: Ty | None = None) -> str:
        s = scrutinee_ty or self.scrutinee_ty()
        scrutinee = self.expr(s, ctx.down())
        clauses = self.clauses(s, ctx, scrutinee)
        parts: list[str] = []
        for i, (pattern, binds) in enumerate(clauses):
            parts += [pattern, body(ctx.with_env(*binds), i)]
        return f"(match {scrutinee} " + " ".join(parts) + ")"

    def scrutinee_ty(self) -> Ty:
        roll = self.rng.random()
        if roll < 0.3:
            return opt(self.encodable_inner(1))
        if roll < 0.45:
            return res(self.rand_ty(1), self.pick([STRING, CALL_ERROR, INT]))
        if roll < 0.55:
            return res(self.rand_ty(1), CALL_ERROR)
        if roll < 0.65:
            return self.pick([LEVEL, KIND, SHAPE])
        if roll < 0.75:
            return KIND
        if roll < 0.85:
            return INT
        if roll < 0.92:
            return STRING
        return BOOL

    def clauses(self, s: Ty, ctx: Ctx, scrutinee: str) -> list[Clause]:
        out: list[Clause] = []
        h = s.head
        fallback_needed = True
        if h == "Option":
            inner = s.args[0]
            partial: list[Clause] = []
            if inner in {INT, STRING, BOOL}:
                literals = dict.fromkeys(self.literal_pattern(inner) for _ in range(self.rng.randint(0, 2)))
                partial.extend((f"(some {lit})", []) for lit in literals)
            cover_some = self.cover_pattern(inner, ctx)
            some = (f"(some {cover_some[0]})", cover_some[1])
            none: Clause = ("none", [])
            if (
                scrutinee.isidentifier()
                and scrutinee in ctx.names()
                and self.chance(0.4)
                and not partial
            ):
                return [none, ("_", [Binding(scrutinee, inner)])]
            rest = self.pick([[some, none], [none, some], [some], [none]])
            out = partial + rest
            fallback_needed = len(rest) < 2
        elif h == "Result":
            ok_p = self.cover_pattern(s.args[0], ctx)
            err_p = self.cover_pattern(s.args[1], ctx)
            ok = (f"(ok {ok_p[0]})", ok_p[1])
            err = (f"(err {err_p[0]})", err_p[1])
            partials: list[Clause] = []
            if s.args[0] in {INT, STRING, BOOL} and self.chance(0.3):
                partials.append((f"(ok {self.literal_pattern(s.args[0])})", []))
            if s.args[1] in {INT, STRING, BOOL} and self.chance(0.3):
                partials.append((f"(err {self.literal_pattern(s.args[1])})", []))
            rest = self.pick([[ok, err], [err, ok], [ok], [err]])
            out = partials + rest
            fallback_needed = len(rest) < 2
        elif h == "Enum":
            variants = list(self.enums[s.name])
            self.rng.shuffle(variants)
            covered = 0
            for variant, payload in variants:
                if self.chance(0.85):
                    covered += 1
                    if payload is None:
                        out.append((variant, []))
                    else:
                        src, bs = self.cover_pattern(payload, ctx)
                        if payload in {INT, STRING} and self.chance(0.3):
                            out.append((f"({variant} {self.literal_pattern(payload)})", []))
                        out.append((f"({variant} {src})", bs))
            fallback_needed = covered < len(variants)
        elif h in {"Int", "String"}:
            lits = list(dict.fromkeys(self.literal_pattern(s) for _ in range(self.rng.randint(1, 3))))
            out = [(lit, []) for lit in lits]
        elif h == "Bool":
            lits = self.pick([["true", "false"], ["false", "true"], ["true"], ["false"]])
            out = [(lit, []) for lit in lits]
            fallback_needed = len(lits) < 2
        if fallback_needed or self.chance(0.12):
            if self.chance(0.5):
                out.append(("_", []))
            else:
                name = self.name(ctx)
                out.append((name, [Binding(name, s)]))
        return out

    def backoff(self) -> str:
        first, cap = sorted(self.rng.choices(range(len(DURATIONS)), k=2))
        return f"[{DURATIONS[first]} {DURATIONS[cap]}]"

    def retry_form(self, ty: Ty, ctx: Ctx) -> str:
        roll = self.rng.random()
        if roll < 0.35:
            opts = f"{{:backoff {self.backoff()} :within {self.pick(DURATIONS)}}}"
        else:
            delays = " ".join(self.pick(DURATIONS) for _ in range(self.rng.randint(0, 4)))
            opts = f"{{:delays [{delays}]}}"
        non_idem = [c for c in self.caps if c.write and not c.idempotent and c.output == ty]
        if non_idem and self.chance(0.4) and ctx.writes == "any":
            cap = self.pick(non_idem)
            kinds = " ".join(self.rng.sample(REFUSAL, self.rng.randint(1, 3)))
            if self.chance(0.1):
                kinds += " " + self.pick(["Timeout", "Failed"])
            call = self.cap_call(cap, ctx.mode("none"))
            if self.chance(0.4):
                call = f"(let [{self.fresh()} {self.expr(self.rand_ty(1), ctx.mode('none').down())}] {call})"
            return f"(retry {{:delays [{self.pick(DURATIONS)}] :on [{kinds}]}} {call})"
        if self.chance(0.5):
            kinds = " ".join(self.rng.sample(ALL_KINDS, self.rng.randint(1, 4)))
            opts = opts[:-1] + f" :on [{kinds}]}}"
        mode = "any" if self.chance(0.05) else "idempotent"
        if ctx.writes == "none":
            mode = "none"
        return f"(retry {opts} {self.expr(ty, ctx.mode(mode))})"

    def wait_form(self, ty: Ty, ctx: Ctx) -> str:
        if self.chance(0.5):
            opts = f"{{:within {self.pick(DURATIONS)} :every {self.pick(DURATIONS)}}}"
        else:
            opts = f"{{:within {self.pick(DURATIONS)} :backoff {self.backoff()}}}"
        pc = ctx.mode("none")
        if ty == STRING and self.chance(0.5):
            pred = '(fn [s] (= s "done"))'
            body = '(call job.state {:id "j"})'
            return f"(wait-until {opts} {pred} {body})"
        return f"(wait-until {opts} {self.fn_arg([ty], BOOL, pc)} {self.expr(ty, pc)})"

    def loop_form(self, ty: Ty, ctx: Ctx) -> str:
        binders: list[Binding] = []
        parts: list[str] = []
        counter = self.fresh("i")
        inits: list[tuple[Binding, str]] = []
        if self.chance(0.8):
            inits.append((Binding(counter, INT), self.pick(["0", "0", "1", "-1"])))
        for _ in range(self.rng.randint(0, 2)):
            t = self.rand_ty(1)
            name = self.fresh("acc")
            if t.head == "Option":
                init = "none"
            elif t.head == "List" and self.chance(0.5):
                init = "[]"
            else:
                init = self.expr(t, ctx.down())
            inits.append((Binding(name, t), init))
        if not inits:
            inits.append((Binding(counter, INT), "0"))
        for b, init in inits:
            binders.append(b)
            parts += [b.name, init]
        maximum = self.pick([1, 2, 3, 4, 5, 8, 20])
        inner = Ctx((*ctx.env, *binders), ctx.depth - 1, ctx.writes, tuple(binders))
        saved = self.finals
        for _ in range(4):
            self.reset_finals()
            body = self.tail(ty, inner)
            if self.finals:
                break
        else:
            body = f"(if {self.expr(BOOL, inner.down(2))} {self.expr(ty, inner.down(2))} {body})"
        self.finals = saved
        return f"(loop {{:max {maximum}}} [{' '.join(parts)}] {body})"

    def reset_finals(self) -> None:
        self.finals = 0

    def recur_form(self, ctx: Ctx) -> str:
        assert ctx.loop is not None
        plain = Ctx(tuple(b for b in ctx.env), ctx.depth - 1, ctx.writes, None)
        args = []
        for b in ctx.loop:
            assert b.ty is not None
            if b.name.startswith("i") and b.ty == INT and self.chance(0.7):
                args.append(f"(+ {b.name} 1)")
            elif b.ty.head == "Option" and self.chance(0.4):
                args.append(self.expr(b.ty.args[0], plain))
            else:
                args.append(self.expr(b.ty, plain))
        return "(recur " + " ".join(args) + ")"

    def tail(self, ty: Ty, ctx: Ctx) -> str:
        if ctx.depth <= 0 or self.chance(0.1):
            self.finals += 1
            return self.expr(ty, Ctx(ctx.env, ctx.depth, ctx.writes, None))
        c = ctx.down()

        def fin(cc: Ctx) -> str:
            self.finals += 1
            return self.expr(ty, Ctx(cc.env, cc.depth, cc.writes, None))

        def sub(cc: Ctx, _i: int) -> str:
            return self.tail_branch(ty, cc, fin)

        return self.weighted(
            [
                (3.0, lambda: self.if_form(ty, c, sub)),
                (1.5, lambda: self.match_form(ty, c, sub)),
                (1.0, lambda: self.cond_form(ty, c, sub)),
                (1.0, lambda: self.let_form(ty, c, sub)),
                (1.0, lambda: self.recur_form(c)),
            ]
        )

    def tail_branch(self, ty: Ty, ctx: Ctx, fin: Callable[[Ctx], str]) -> str:
        if self.chance(0.5):
            return self.recur_form(ctx)
        if self.chance(0.3) and ctx.depth > 0:
            return self.tail(ty, ctx.down())
        return fin(ctx)

    def poly_body(self, x: str, depth: int) -> str:
        if depth <= 0 or self.chance(0.15):
            return x

        def inner() -> str:
            return self.poly_body(x, depth - 1)

        shapes: list[Callable[[], str]] = [
            lambda: f"[{inner()}]",
            lambda: f"[{x} {inner()}]",
            lambda: f"[{inner()} {inner()}]",
            lambda: f"{{:a {inner()}}}",
            lambda: f"{{:a {inner()} :b {inner()}}}",
            lambda: f"(some {inner()})",
            lambda: f"(ok {inner()})",
            lambda: f"(err {inner()})",
            lambda: f"(if true {inner()} {inner()})",
            lambda: f"(let [y {inner()}] {inner()})",
            lambda: f"(first [{inner()}])",
            lambda: f"(nth [{inner()} {inner()}] 0)",
            lambda: f"(concat [{inner()}] [{inner()}])",
            lambda: f'(assoc {{}} "k" {inner()})',
            lambda: f'(into-map [["a" {inner()}]])',
            lambda: f'(vals (assoc {{}} "k" {inner()}))',
            lambda: f"(flatten [[{inner()}]])",
            lambda: f'(entries (into-map [["a" {inner()}]]))',
            lambda: f"(get {{:a {inner()}}} :a)",
            lambda: f"(map identity [{inner()}])",
            lambda: f"(rest [{inner()}])",
            lambda: f"(take 1 [{inner()}])",
            lambda: f"(reduce (fn [acc y] [acc y]) {inner()} [{inner()}])",
            lambda: f"(match {inner()} (some y) {inner()} none {inner()})",
            lambda: f"(attempt {inner()})",
            lambda: f"(retry {{:delays [1s]}} {inner()})",
            lambda: f"(loop {{:max 2}} [z {inner()}] (if true z (recur {inner()})))",
        ]
        return self.pick(shapes)()

    def poly_program(self) -> str:
        body = self.poly_body("x", self.rng.randint(1, 4))
        source = self.pick(["[]", "[]", "[]", "(call store.list {})", "[1]", "[[]]", '[(some "a")]'])
        head = self.pick(["map", "map", "keep", "filter", "any?", "all?"])
        if head == "keep":
            body = f"(some {body})"
        elif head in {"filter", "any?", "all?"}:
            body = f"(let [y {body}] true)"
        program = f"({head} (fn [x] {body}) {source})"
        if self.chance(0.4):
            program = f"(let [f (fn [x] {self.poly_body('x', 2)})] (map f {source}))"
        wrap = self.pick(["{}", "(entries (into-map {}))", "(into-map {})", "(flatten {})", "(first {})", "(count {})"])
        return wrap.format(program)

    def random_float(self) -> str:
        roll = self.rng.random()
        if roll < 0.3:
            return self.float_literal()
        if roll < 0.6:
            value = self.rng.uniform(-1000, 1000)
        elif roll < 0.8:
            value = self.rng.random() * 10 ** self.rng.randint(-320, 308) * self.rng.choice([-1, 1])
        else:
            value = struct.unpack("<d", self.rng.getrandbits(64).to_bytes(8, "little"))[0]
            if value != value or value in {float("inf"), float("-inf")}:
                value = 0.5
        text = repr(float(value)).replace("e+", "e")
        return text if "." in text or "e" in text else text + ".0"

    def number(self, ty: Ty, depth: int) -> str:
        if depth <= 0 or self.chance(0.25):
            return self.random_float() if ty == FLOAT else self.int_literal()
        a, b = self.number(ty, depth - 1), self.number(ty, depth - 1)
        if ty == FLOAT:
            return self.pick(
                [
                    f"(+ {a} {b})", f"(- {a} {b})", f"(* {a} {b})", f"(/ {a} {b})", f"(- {a})",
                    f"(float {self.number(INT, depth - 1)})",
                ]
            )
        return self.pick(
            [
                f"(+ {a} {b})", f"(- {a} {b})", f"(* {a} {b})", f"(quot {a} {b})", f"(rem {a} {b})", f"(- {a})",
                f"(round {self.number(FLOAT, depth - 1)})", f"(floor {self.number(FLOAT, depth - 1)})",
                f"(ceil {self.number(FLOAT, depth - 1)})", f"(count [{a} {b}])",
            ]
        )

    def numeric_program(self) -> str:
        ty = self.pick([INT, FLOAT])
        count = self.rng.randint(1, 4)
        items = []
        for _ in range(count):
            kind = self.rng.random()
            if kind < 0.5:
                items.append(self.number(ty, self.rng.randint(1, 3)))
            elif kind < 0.7:
                comparison = self.pick(["<", "<=", ">", ">=", "=", "not="])
                items.append(f"({comparison} {self.number(ty, 1)} {self.number(ty, 1)})")
            else:
                items.append(f"(str {self.number(ty, 2)} {self.number(self.pick([INT, FLOAT]), 1)})")
        if count == 1:
            return items[0]
        if self.chance(0.3):
            return f"(sort-by (fn [x] x) [{' '.join(self.number(ty, 1) for _ in range(count + 1))}])"
        return "[" + " ".join(items) + "]"

    def program(self) -> str:
        if self.chance(0.1):
            return self.poly_program()
        if self.chance(0.1):
            return self.numeric_program()
        ty = self.rand_ty(2)
        return self.expr(ty, Ctx(depth=self.rng.choice([3, 4, 4, 5, 5, 6, 6, 7])))
