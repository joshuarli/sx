from __future__ import annotations

import json
import random
from collections.abc import Callable
from dataclasses import dataclass, field
from pathlib import Path

from .case import Json

DECLARATIONS = Path(__file__).resolve().parent / "declarations.json"
KINDS = ("Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed")
NAMES = ("alpha", "Beta", "gamma delta", "é", "😀 smile", "n3", " padded ", "", "x", "Zed", "42", "a,b")
STRINGS = ("a", "b", "alpha", "x y", "é", "😀", "a😀b", "", "10", "Zed", "i1", "n7", " t ", "-5")
PRICES = (0.5, 1.5, 2.0, 10.25, 0.1, 3.0, 7.75)
NOTES = ("hello", "12", "multi\nline", "", "😀", "3.5")
INT_LITERALS = ("0", "1", "2", "3", "5", "7", "10", "100", "1000")
TRAP_STATEMENTS = (
    "const trapped = ints[ints.length + 3];",
    "const trapped = 9007199254740990 + ints.length * 4;",
    "const trapped = 1 / (ints.length - ints.length);",
    "const trapped = parseInt(strs.join(\"\") + \"x\" + \"\", 10) + parseInt(\"z\", 10);",
    "const trapped = Math.floor(1e20 * 3);",
    "const trapped = \"a😀b\".slice(0, 2);",
    "const trapped = ints.length % (ints.length - ints.length);",
    "const [trapA, trapB, trapC] = [ints.length, 1];",
)


@dataclass(slots=True)
class World:
    rng: random.Random
    seed: int
    items: list[Json] = field(default_factory=list)
    injected: int = 0

    def __post_init__(self) -> None:
        count = 5
        self.items = [
            {
                "id": f"i{n}", "name": self.rng.choice(NAMES), "qty": self.rng.choice((-3, 0, 1, 2, 3, 5, 9)),
                "price": self.rng.choice(PRICES), "note": self.rng.choice((None, "n", "", "memo")),
            }
            for n in range(count)
        ]

    def error(self) -> Json:
        self.injected += 1
        kind = KINDS[(self.seed + self.injected) % len(KINDS)] if self.rng.random() < 0.6 else self.rng.choice(KINDS)
        return {"error": {"kind": kind, "message": f"{kind.lower()} #{self.injected}"}}

    def results(self, capability: str, request: Json, write: bool) -> list[Json]:
        outputs = self.outputs(capability, request)
        results: list[Json] = [{"error": o["error"]} if "error" in o else {"output": o["output"]} for o in outputs]
        if not results:
            return []
        if self.rng.random() < 0.07:
            results.insert(0, self.error())
        if write:
            results += [results[-1]] * 3
        return results

    def outputs(self, capability: str, request: Json) -> list[Json]:
        by_id = {item["id"]: item for item in self.items}
        match capability:
            case "items.list":
                return [{"output": self.items}]
            case "items.get":
                item = by_id.get(request["id"])
                missing = {"error": {"kind": "NotFound", "message": f"no item {request['id']}"}}
                return [{"output": item}] if item else [missing]
            case "items.find":
                limit = request.get("limit")
                found = [i["id"] for i in self.items if request["query"] in i["name"]]
                return [{"output": found if limit is None else found[:limit]}]
            case "items.count":
                return [{"output": len(self.items)}]
            case "items.review":
                item = by_id.get(request["id"])
                if item is None:
                    return [{"error": {"kind": "NotFound", "message": f"no item {request['id']}"}}]
                if item["qty"] <= 0:
                    return [{"output": "Missing"}]
                if item["qty"] % 2 == 0:
                    return [{"output": {"tag": "Score", "value": item["qty"] * 10}}]
                return [{"output": {"tag": "Flagged", "value": {"reason": item["name"], "level": item["qty"]}}}]
            case "items.update":
                item = by_id.get(request["id"])
                if item is None:
                    return [{"error": {"kind": "NotFound", "message": f"no item {request['id']}"}}]
                return [{"output": {**item, "qty": request["qty"]}}]
            case "items.create":
                return [{"output": {"id": f"i{len(self.items) + 1}"}}]
            case "stats.mean":
                prices = [by_id[i]["price"] for i in request["ids"] if i in by_id]
                return [{"output": sum(prices) / len(prices) if prices else 0.0}]
            case "notes.read":
                return [{"output": self.rng.choice(NOTES)}]
            case "notes.write":
                return [{"output": None}]
            case "pages.fetch":
                step = int(request["cursor"][1:]) if request.get("cursor") else 0
                pool = [i["id"] for i in self.items] or ["p"]
                page = {"items": pool[step : step + 2], "next": f"p{step + 2}" if step + 2 < len(pool) else None}
                return [{"output": page}]
            case "jobs.status":
                states = [("running", 0), ("running", 40), ("done", 100)]
                return [{"output": {"state": s, "progress": p}} for s, p in states[self.rng.randint(0, 2) :]]
            case _:
                raise ValueError(f"no world for {capability}")


WRITES = frozenset({"items.update", "items.create", "notes.write"})
TYPES = ("int", "float", "str", "bool", "item", "items", "strs", "ints", "opt", "rec")


@dataclass(slots=True)
class Var:
    name: str
    type: str
    mutable: bool = False


@dataclass(slots=True)
class Writer:
    rng: random.Random
    counter: int = 0
    lines: list[str] = field(default_factory=list)
    scopes: list[list[Var]] = field(default_factory=lambda: [[]])
    loop_depth: int = 0
    helpers: list[str] = field(default_factory=list)

    def fresh(self, stem: str) -> str:
        self.counter += 1
        return f"{stem}{self.counter}"

    def declare(self, stem: str, type_: str, mutable: bool = False) -> Var:
        var = Var(self.fresh(stem), type_, mutable)
        self.scopes[-1].append(var)
        return var

    def vars_of(self, *types: str, mutable: bool | None = None) -> list[Var]:
        return [
            v for scope in self.scopes for v in scope if v.type in types and (mutable is None or v.mutable == mutable)
        ]

    def emit(self, text: str) -> None:
        self.lines.append("  " * (len(self.scopes) - 1) + text)

    def block(self, header: str, body: Callable[[], None], footer: str = "}") -> None:
        self.emit(header)
        self.scopes.append([])
        body()
        self.scopes.pop()
        self.emit(footer)

    def pick(self, *options: Callable[[], str]) -> str:
        return self.rng.choice(options)()

    def int_(self, d: int = 2) -> str:
        r = self.rng
        names = [v.name for v in self.vars_of("int")]
        leaves: list[Callable[[], str]] = [lambda: r.choice(INT_LITERALS)]
        if names:
            leaves += [lambda: r.choice(names)] * 3
        if d <= 0:
            return self.pick(*leaves)
        options: list[Callable[[], str]] = [
            *leaves,
            lambda: f"({self.int_(d - 1)} {r.choice('+-*')} {self.int_(d - 1)})",
            lambda: f"({self.int_(d - 1)} % {r.choice(('2', '3', '7', '-4'))})",
            lambda: f"Math.{r.choice(('floor', 'ceil', 'round', 'trunc'))}({self.float_(d - 1)})",
            lambda: f"Math.{r.choice(('max', 'min'))}({self.int_(d - 1)}, {self.int_(d - 1)})",
            lambda: f"Math.abs({self.int_(d - 1)})",
            lambda: f"{self.list_name('strs', 'ints', 'items')}.length",
            lambda: f"{self.str_(d - 1)}.length",
        ]
        if self.vars_of("item"):
            options.append(lambda: f"{r.choice(self.vars_of('item')).name}.qty")
        if self.vars_of("ints"):
            options += [
                lambda: f"{r.choice(self.vars_of('ints')).name}.reduce((a, b) => a + b, 0)",
                lambda: f"{r.choice(self.vars_of('ints')).name}[{r.choice(('0', '1', '2'))}]",
            ]
        options.append(lambda: f"parseInt(String({self.int_(d - 1)}), 10)")
        options.append(lambda: f"{self.strs_(0)}.indexOf({self.str_(0)})")
        return self.pick(*options)

    def list_name(self, *types: str) -> str:
        found = self.vars_of(*types)
        return self.rng.choice(found).name if found else "[1, 2, 3]"

    def float_(self, d: int = 2) -> str:
        r = self.rng
        names = [v.name for v in self.vars_of("float")]
        leaves: list[Callable[[], str]] = [lambda: r.choice(("1.5", "0.25", "2.0", "10.5", "0.1", "3e2"))]
        if names:
            leaves += [lambda: r.choice(names)] * 3
        if d <= 0:
            return self.pick(*leaves)
        options: list[Callable[[], str]] = [
            *leaves,
            lambda: f"({self.float_(d - 1)} {r.choice('+-*')} {self.float_(d - 1)})",
            lambda: f"({self.int_(d - 1)} {r.choice('+-*')} {self.float_(d - 1)})",
            lambda: f"({self.int_(d - 1)} / {r.choice(('2', '3', '4', '8'))})",
            lambda: f"({self.float_(d - 1)} / {r.choice(('2', '0.5', '4'))})",
            lambda: f"Math.abs({self.float_(d - 1)})",
        ]
        if self.vars_of("item"):
            options.append(lambda: f"{r.choice(self.vars_of('item')).name}.price")
        return self.pick(*options)

    def str_(self, d: int = 2) -> str:
        r = self.rng
        names = [v.name for v in self.vars_of("str")]
        leaves: list[Callable[[], str]] = [lambda: json.dumps(r.choice(STRINGS), ensure_ascii=False)]
        if names:
            leaves += [lambda: r.choice(names)] * 3
        if d <= 0:
            return self.pick(*leaves)
        options: list[Callable[[], str]] = [
            *leaves,
            lambda: f"({self.str_(d - 1)} + {self.pick(lambda: self.str_(0), lambda: self.int_(0))})",
            lambda: f"`${{{self.int_(d - 1)}}}-${{{self.str_(d - 1)}}}`",
            lambda: f"String({self.int_(d - 1)})",
            lambda: f"({self.str_(d - 1)}).trim()",
            lambda: f"({self.str_(d - 1)}).toUpperCase()",
            lambda: f"({self.str_(d - 1)}).slice(0, {r.choice(('1', '2', '3'))})",
            lambda: f"({self.str_(d - 1)}).padStart({r.choice(('3', '5'))}, \"0\")",
            lambda: f"({self.str_(d - 1)}).replace(\"a\", \"_\")",
            lambda: f"{self.strs_(d - 1)}.join({json.dumps(r.choice((',', '-', '')))})",
            lambda: f"({self.opt_(d - 1)} ?? {self.str_(0)})",
            lambda: f"({self.bool_(d - 1)} ? {self.str_(d - 1)} : {self.str_(d - 1)})",
        ]
        if self.vars_of("item"):
            options += [
                lambda: f"{r.choice(self.vars_of('item')).name}.name",
                lambda: f"{r.choice(self.vars_of('item')).name}.id",
            ]
        return self.pick(*options)

    def opt_(self, d: int = 1) -> str:
        r = self.rng
        options: list[Callable[[], str]] = [lambda: "null", lambda: f"({self.bool_(0)} ? {self.str_(0)} : null)"]
        found = self.vars_of("opt")
        if found:
            options += [lambda: r.choice(found).name] * 3
        if self.vars_of("item"):
            options += [lambda: f"{r.choice(self.vars_of('item')).name}.note"] * 3
        return self.pick(*options)

    def bool_(self, d: int = 2) -> str:
        r = self.rng
        names = [v.name for v in self.vars_of("bool")]
        leaves: list[Callable[[], str]] = [lambda: r.choice(("true", "false"))]
        if names:
            leaves += [lambda: r.choice(names)] * 2
        if d <= 0:
            return self.pick(*leaves)
        options: list[Callable[[], str]] = [
            *leaves,
            lambda: f"({self.int_(d - 1)} {r.choice(('<', '<=', '>', '>=', '===', '!=='))} {self.int_(d - 1)})",
            lambda: f"({self.float_(d - 1)} {r.choice(('<', '>=', '==='))} {self.float_(d - 1)})",
            lambda: f"({self.str_(d - 1)} {r.choice(('===', '!==', '<', '>='))} {self.str_(d - 1)})",
            lambda: f"({self.str_(d - 1)}).{r.choice(('includes', 'startsWith', 'endsWith'))}({self.str_(0)})",
            lambda: f"!{self.bool_(d - 1)}",
            lambda: f"({self.bool_(d - 1)} {r.choice(('&&', '||'))} {self.bool_(d - 1)})",
            lambda: f"({self.opt_(0)} {r.choice(('== null', '!= null'))})",
            lambda: f"{self.strs_(d - 1)}.includes({self.str_(0)})",
            lambda: f"Number.isInteger({self.float_(d - 1)})",
            lambda: (
                f"{self.ints_(d - 1)}.{r.choice(('some', 'every'))}"
                f"((n) => n {r.choice(('>', '<'))} {r.choice(INT_LITERALS)})"
            ),
        ]
        return self.pick(*options)

    def cond(self) -> str:
        r = self.rng
        containers = self.vars_of("rec", "ints", "strs")
        extra: list[Callable[[], str]] = [
            lambda: self.str_(1),
            lambda: self.int_(1),
            lambda: self.opt_(1),
            lambda: r.choice(containers).name if containers else "true",
        ]
        return self.pick(lambda: self.bool_(2), lambda: self.bool_(2), lambda: self.bool_(1), *extra)

    def strs_(self, d: int = 1) -> str:
        r = self.rng
        found = [v.name for v in self.vars_of("strs")]
        options: list[Callable[[], str]] = [lambda: "[" + ", ".join(self.str_(0) for _ in range(r.randint(1, 3))) + "]"]
        if found:
            options += [lambda: r.choice(found)] * 3
        if d > 0:
            options += [
                lambda: f"({self.str_(0)}).split({json.dumps(r.choice((',', ' ', '-')))})",
                lambda: f"[...{self.strs_(d - 1)}, {self.str_(d - 1)}]",
                lambda: f"{self.strs_(d - 1)}.slice(0, {r.choice(('1', '2'))})",
                lambda: f"{self.strs_(d - 1)}.filter((s) => s {r.choice(('!==', '==='))} {self.str_(0)})",
                lambda: f"{self.strs_(d - 1)}.map((s) => s.toUpperCase())",
                lambda: f"{self.strs_(d - 1)}.concat({self.strs_(0)})",
            ]
        if self.vars_of("items"):
            options.append(lambda: f"{r.choice(self.vars_of('items')).name}.map((it) => it.{r.choice(('name', 'id'))})")
        return self.pick(*options)

    def ints_(self, d: int = 1) -> str:
        r = self.rng
        found = [v.name for v in self.vars_of("ints")]
        options: list[Callable[[], str]] = [
            lambda: "[" + ", ".join(r.choice(INT_LITERALS) for _ in range(r.randint(1, 4))) + "]"
        ]
        if found:
            options += [lambda: r.choice(found)] * 3
        if d > 0:
            options += [
                lambda: f"{self.ints_(d - 1)}.filter((n) => n {r.choice(('>', '<', '!=='))} {r.choice(INT_LITERALS)})",
                lambda: f"{self.ints_(d - 1)}.map((n) => n * {r.choice(('2', '3'))})",
                lambda: f"{self.strs_(d - 1)}.map((s) => s.length)",
                lambda: f"{self.ints_(d - 1)}.slice({r.choice(('1', '2'))})",
            ]
        if self.vars_of("items"):
            options.append(lambda: f"{r.choice(self.vars_of('items')).name}.map((it) => it.qty)")
        return self.pick(*options)

    def items_(self) -> str:
        found = [v.name for v in self.vars_of("items")]
        r = self.rng
        base = r.choice(found)
        return self.pick(
            lambda: base,
            lambda: base,
            lambda: f"{base}.filter((it) => it.qty {r.choice(('>', '<=', '!=='))} {r.choice(('0', '1', '2')) })",
            lambda: f"{base}.slice(0, {r.choice(('1', '2', '3'))})",
        )

    def of_type(self, type_: str) -> str:
        match type_:
            case "int":
                return self.int_()
            case "float":
                return self.float_()
            case "str":
                return self.str_()
            case "bool":
                return self.bool_()
            case "opt":
                return self.opt_()
            case "strs":
                return self.strs_()
            case "ints":
                return self.ints_()
            case "rec":
                return f"{{ n: {self.int_(1)}, label: {self.str_(1)} }}"
            case _:
                raise ValueError(type_)


def item_id(w: Writer) -> str:
    items = w.vars_of("item")
    pool = [lambda: json.dumps(f"i{w.rng.choice((0, 0, 1, 1, 2, 2, 2, 3, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 4, 9))}")]
    if items:
        pool += [lambda: f"{w.rng.choice(items).name}.id"]
    return w.pick(*pool)


def optional_name(w: Writer) -> str:
    var = w.declare("ov", "narrowable")
    items = w.vars_of("item")
    source = w.pick(
        lambda: f"({w.bool_(1)} ? {w.str_(0)} : null)",
        lambda: f"({w.int_(1)} > 1 ? {w.str_(0)} : null)",
        *([lambda: f"{w.rng.choice(items).name}.note"] * 3 if items else []),
    )
    w.emit(f"const {var.name} = {source};")
    return var.name


def narrowing(w: Writer, depth: int) -> None:
    r = w.rng
    o = optional_name(w)
    k = w.fresh("k")
    match r.choice(("exit", "exit", "throw", "value", "value", "else", "truthy", "nested", "pair", "function", "all")):
        case "exit":
            guard = r.choice((f"{o} == null", f"!{o}"))
            leave = r.choice(("continue", "break"))
            w.emit(f"for (const {k} of [1, 2, 3]) {{")
            w.emit(f"  if ({guard}) {{ {leave}; }}")
            w.emit(f"  console.log({o}.length, {k});")
            w.emit("}")
        case "throw":
            w.emit("try {")
            w.emit(f"  if ({o} == null) {{ throw new Error(\"absent\"); }}")
            w.emit(f"  console.log({o}.length);")
            w.emit("} catch (e) {")
            w.emit("  console.log(e.message);")
            w.emit("}")
        case "value":
            shape = r.choice(
                (
                    ("int", f"{o} == null ? {w.int_(0)} : {o}.length"),
                    ("int", f"{o} != null ? {o}.length : {w.int_(0)}"),
                    ("int", f"{o} ? {o}.length : {w.int_(0)}"),
                    ("bool", f"{o} != null && {o}.length > {r.choice(('0', '1', '2'))}"),
                    ("bool", f"{o} == null || {o}.length < {r.choice(('1', '2', '3'))}"),
                    ("bool", f"{o} != null && {o}.startsWith({w.str_(0)})"),
                    ("str", f"{o} == null ? \"none\" : {o}.toUpperCase()"),
                )
            )
            var = w.declare("nv", shape[0])
            w.emit(f"const {var.name} = {shape[1]};")
        case "else":
            w.emit(f"if ({o} == null) {{")
            w.emit("  console.log(\"none\");")
            w.emit("} else {")
            w.emit(f"  console.log({o}.length);")
            w.emit("}")
        case "truthy":
            w.emit(f"if ({o}) {{")
            w.emit(f"  console.log({o}.length, \"truthy\");")
            w.emit("} else {")
            w.emit(f"  console.log({o} === null ? 0 : 1);".replace(f"{o} === null ? 0 : 1", f"{o} == null"))
            w.emit("}")
            w.emit(f"if ({o} && {o}.length > 1) {{ console.log({o}); }}")
        case "nested":
            w.emit(f"if ({o} != null) {{")
            w.emit(f"  if ({o} != null) {{ console.log({o}.length); }}")
            w.emit(f"  console.log({o} != null ? {o}.length : -1);")
            w.emit("}")
        case "pair":
            other = optional_name(w)
            w.emit(f"if ({o} != null && {other} != null) {{")
            w.emit(f"  console.log({o}.length + {other}.length);")
            w.emit("}")
            w.emit(f"for (const {k} of [1, 2]) {{")
            w.emit(f"  if ({o} == null || {other} == null) {{ continue; }}")
            w.emit(f"  console.log({o}.length - {other}.length, {k});")
            w.emit("}")
        case "function":
            name = w.fresh("narrowed")
            parameter = w.fresh("p")
            fallback = r.choice(("-1", "0", "99"))
            w.helpers.append(
                f"function {name}({parameter}: string | null): number {{\n"
                f"  if ({parameter} == null) {{ return {fallback}; }}\n"
                f"  return {parameter}.length;\n}}"
            )
            var = w.declare("hv", "int")
            w.emit(f"const {var.name} = {name}({o});")
        case _:
            w.emit(f"const {w.fresh('nf')} = [{o}, null].map((x) => (x == null ? 0 : x.length));")


def variants(w: Writer, depth: int) -> None:
    r = w.rng
    reviews = w.vars_of("review")
    if not reviews or r.random() < 0.3:
        fresh = w.declare("review", "review")
        w.emit(f"const {fresh.name} = await tools.items.review({{ id: {item_id(w)} }});")
        reviews = [fresh]
    v = r.choice(reviews).name
    k = w.fresh("k")
    match r.choice(("str", "int", "exit", "tag", "typeof", "early")):
        case "str":
            var = w.declare("rs", "str")
            w.emit(
                f'const {var.name} = {v} === "Missing" ? "m" : {v}.tag === "Score" ? String({v}.value) : {v}.value.reason;'
            )
        case "int":
            var = w.declare("ri", "int")
            w.emit(f'const {var.name} = {v} === "Missing" ? 0 : {v}.tag === "Flagged" ? {v}.value.level : {v}.value;')
        case "exit":
            w.emit(f"for (const {k} of [1, 2]) {{")
            w.emit(f'  if ({v} === "Missing") {{ continue; }}')
            w.emit(f'  if ({v}.tag === "Score") {{ console.log({v}.value + {k}); }} else {{ console.log({v}.value.reason, {v}.value.level); }}')
            w.emit("}")
        case "tag":
            var = w.declare("rt", "bool")
            w.emit(f'const {var.name} = {v} !== "Missing" && {v}.tag === "{r.choice(("Score", "Flagged"))}";')
        case "typeof":
            var = w.declare("ro", "str")
            w.emit(f"const {var.name} = typeof {v};")
        case _:
            w.emit(f"for (const {k} of [1, 2]) {{")
            w.emit(f'  if ({v} === "Missing" || {v}.tag !== "Flagged") {{ break; }}')
            w.emit(f"  console.log({v}.value.reason + {k});")
            w.emit("}")


def statements(w: Writer, count: int, depth: int) -> None:
    for _ in range(count):
        statement(w, depth)


def statement(w: Writer, depth: int) -> None:
    r = w.rng
    choices: list[Callable[[], None]] = [
        lambda: declare_value(w),
        lambda: declare_value(w),
        lambda: call_read(w),
        lambda: call_read(w),
        lambda: log(w),
        lambda: try_catch(w, depth),
        lambda: parallel(w),
        lambda: bounded(w),
        lambda: call_write(w),
        lambda: narrowing(w, depth),
        lambda: variants(w, depth),
    ]
    if depth > 0:
        choices += [
            lambda: branch(w, depth),
            lambda: counted_loop(w, depth),
            lambda: helper_call(w),
            lambda: accumulate_loop(w, depth),
            lambda: accumulate_loop(w, depth),
        ]
    r.choice(choices)()


def declare_value(w: Writer) -> None:
    type_ = w.rng.choice(("int", "int", "float", "str", "str", "bool", "opt", "strs", "ints", "rec"))
    expression = w.of_type(type_)
    mutable = type_ in ("int", "float", "str") and w.rng.random() < 0.5
    var = w.declare(type_[0] + "v", type_, mutable)
    w.emit(f"{'let' if mutable else 'const'} {var.name} = {expression};")


def call_read(w: Writer) -> None:
    r = w.rng
    kind = r.choice(("list", "list", "get", "find", "count", "mean", "note", "item-from-list", "review"))
    match kind:
        case "review":
            var = w.declare("review", "review")
            w.emit(f"const {var.name} = await tools.items.review({{ id: {item_id(w)} }});")
        case "list":
            var = w.declare("items", "items")
            w.emit(f"const {var.name} = await tools.items.list({{}});")
        case "get":
            expression = item_id(w)
            var = w.declare("item", "item")
            w.emit(f"const {var.name} = await tools.items.get({{ id: {expression} }});")
        case "find":
            var = w.declare("ids", "strs")
            limit = r.choice(("null", "2", "5"))
            query = json.dumps(r.choice(("a", "e", "", "x")))
            w.emit(f"const {var.name} = await tools.items.find({{ query: {query}, limit: {limit} }});")
        case "count":
            var = w.declare("count", "int")
            w.emit(f"const {var.name} = await tools.items.count({{}});")
        case "mean":
            ids = w.strs_(1)
            var = w.declare("mean", "float")
            w.emit(f"const {var.name} = await tools.stats.mean({{ ids: {ids} }});")
        case "note":
            var = w.declare("note", "str")
            path = json.dumps(r.choice(("a.txt", "b.txt")))
            w.emit(f"const {var.name} = await tools.notes.read({{ path: {path} }});")
        case _:
            lists = w.vars_of("items")
            if lists:
                var = w.declare("first", "item")
                w.emit(f"const {var.name} = {r.choice(lists).name}[{r.choice(('0', '1'))}];")


def call_write(w: Writer) -> None:
    r = w.rng
    match r.choice(("update", "create", "write")):
        case "update":
            w.emit(f"await tools.items.update({{ id: {item_id(w)}, qty: {w.int_(1)} }});")
        case "create":
            name, quantity = w.str_(1), w.int_(1)
            var = w.declare("created", "str")
            w.emit(f"const {var.name} = (await tools.items.create({{ name: {name}, qty: {quantity} }})).id;")
        case _:
            path = json.dumps(r.choice(("a.txt", "out.txt")))
            w.emit(f"await tools.notes.write({{ path: {path}, text: {w.str_(1)} }});")


def log(w: Writer) -> None:
    arguments = ", ".join(
        w.pick(
            lambda: w.str_(1), lambda: w.int_(1), lambda: w.bool_(1), lambda: w.float_(1), lambda: w.strs_(1),
            lambda: w.opt_(1),
        )
        for _ in range(w.rng.randint(1, 3))
    )
    w.emit(f"console.log({arguments});")


def branch(w: Writer, depth: int) -> None:
    mutable = w.vars_of("int", "str", "float", mutable=True)
    w.emit(f"if ({w.cond()}) {{")
    w.scopes.append([])
    statements(w, w.rng.randint(1, 2), depth - 1)
    update_mutable(w, mutable)
    w.scopes.pop()
    if w.rng.random() < 0.5:
        w.emit("} else {")
        w.scopes.append([])
        statements(w, w.rng.randint(1, 2), depth - 1)
        update_mutable(w, mutable)
        w.scopes.pop()
    w.emit("}")


def update_mutable(w: Writer, mutable: list[Var]) -> None:
    if not mutable or w.rng.random() < 0.3:
        return
    var = w.rng.choice(mutable)
    r = w.rng
    match var.type:
        case "int":
            w.emit(f"{var.name} {r.choice(('+=', '-=', '*=', '='))} {w.int_(1)};")
        case "float":
            step = r.choice(("1.5", "0.25", "10.5", "0.1"))
            w.emit(f"{var.name} {r.choice(('+=', '-=', '*=', '/='))} {step};" if r.random() < 0.7 else f"{var.name} = {w.float_(1)};")
        case _:
            if r.random() < 0.5:
                w.emit(f"{var.name} += {json.dumps(r.choice(STRINGS))};")
            else:
                w.emit(f"{var.name} = {w.str_(1)};")


def accumulate_loop(w: Writer, depth: int) -> None:
    r = w.rng
    sources = w.vars_of("items", "strs", "ints")
    if not sources:
        return
    source = r.choice(sources)
    element = {"items": "it", "strs": "s", "ints": "n"}[source.type]
    mutable = w.vars_of("int", "str", "float", mutable=True)
    pushed = None
    if r.random() < 0.5:
        pushed = w.declare("out", "ownedstrs")
        w.emit(f"const {pushed.name}: string[] = [];")
    record = None
    if r.random() < 0.3:
        record = w.declare("rec", "rec")
        w.emit(f"const {record.name} = {{ n: 0, label: \"\" }};")
    name = w.fresh(element)

    def body() -> None:
        local = w.declare(element, {"items": "item", "strs": "str", "ints": "int"}[source.type])
        local.name = name
        w.loop_depth += 1
        statements(w, r.randint(0, 2), depth - 1)
        if r.random() < 0.3:
            w.emit(f"if ({w.bool_(1)}) {{ {r.choice(('continue', 'break'))}; }}")
        update_mutable(w, mutable)
        if pushed is not None:
            w.emit(f"{pushed.name}.push({w.str_(1)});")
        if record is not None:
            w.emit(f"{record.name}.n = {record.name}.n + {w.int_(1)};")
            if r.random() < 0.5:
                w.emit(f"{record.name}.label = {w.str_(1)};")
        w.loop_depth -= 1

    w.block(f"for (const {name} of {source.name}) {{", body)


def counted_loop(w: Writer, depth: int) -> None:
    r = w.rng
    mutable = w.vars_of("int", "str", "float", mutable=True)
    bound = r.choice(("2", "3", "4", f"{w.list_name('strs', 'ints', 'items')}.length"))
    name = w.fresh("i")

    def body() -> None:
        w.declare("loopi", "int").name = name
        statements(w, r.randint(0, 2), depth - 1)
        update_mutable(w, mutable)
        if r.random() < 0.3:
            w.emit(f"if ({name} === 1) {{ continue; }}")

    step = r.choice(("i++", "i += 2"))
    w.block(f"for (let {name} = 0; {name} < {bound}; {step.replace('i', name)}) {{", body)


def try_catch(w: Writer, depth: int) -> None:
    r = w.rng
    target = w.declare("got", "str", True)
    w.emit(f"let {target.name} = \"init\";")
    w.emit("try {")
    w.scopes.append([])
    expression = item_id(w)
    value = w.declare("v", "item")
    w.emit(f"const {value.name} = await tools.items.get({{ id: {expression} }});")
    w.emit(f"{target.name} = {value.name}.name;")
    w.scopes.pop()
    w.emit("} catch (e) {")
    w.scopes.append([])
    w.emit(f"{target.name} = {r.choice(('e.message', 'e.kind', 'String(e)', 'JSON.stringify(e)', '`${e.kind}!`'))};")
    w.scopes.pop()
    w.emit("}")


def parallel(w: Writer) -> None:
    r = w.rng
    match r.choice(("pair", "map", "settled", "settled-all")):
        case "pair":
            identifier = item_id(w)
            a, b = w.declare("pa", "item"), w.declare("pb", "int")
            reads = f"[tools.items.get({{ id: {identifier} }}), tools.items.count({{}})]"
            w.emit(f"const [{a.name}, {b.name}] = await Promise.all({reads});")
        case "map":
            ids = w.vars_of("strs")
            source = r.choice(ids).name if ids else "[\"i0\", \"i1\"]"
            var = w.declare("many", "items")
            w.emit(f"const {var.name} = await Promise.all({source}.map((id) => tools.items.get({{ id }})));")
        case "settled":
            a = w.declare("sa", "str")
            reads = 'tools.items.get({ id: "i0" }), tools.notes.read({ path: "a.txt" })'
            w.emit(f"const [{a.name}x, {a.name}y] = await Promise.allSettled([{reads}]);")
            first = f'{a.name}x.status === "fulfilled" ? {a.name}x.value.name'
            w.emit(f'const {a.name} = {first} : {a.name}y.status === "fulfilled" ? {a.name}y.value : "none";')
        case _:
            var = w.declare("states", "strs")
            reads = '["i0", "i1", "zz"].map((id) => tools.items.get({ id }))'
            w.emit(f"const {var.name}raw = await Promise.allSettled({reads});")
            w.emit(f"const {var.name} = {var.name}raw.map((s) => s.status === \"fulfilled\" ? s.value.id : \"i0\");")


def helper_call(w: Writer) -> None:
    r = w.rng
    name = w.fresh("helper")
    parameter = w.fresh("p")
    body = f"{parameter} * {r.choice(('2', '3'))} + {r.choice(('1', '0'))}"
    w.helpers.append(f"function {name}({parameter}: number): number {{\n  return {body};\n}}")
    argument = w.int_(1)
    var = w.declare("hv", "int")
    w.emit(f"const {var.name} = {name}({argument});")


def bounded(w: Writer) -> None:
    r = w.rng
    match r.choice(("retry", "wait", "loop", "parmap", "sleep")):
        case "retry":
            identifier = item_id(w)
            var = w.declare("rt", "item")
            options = r.choice((
                "{ delaysMs: [100, 200] }",
                '{ delaysMs: [50], on: ["Timeout", "NotFound"] }',
                "{ backoffMs: [100, 400], withinMs: 1000 }",
                "{ delaysMs: [] }",
            ))
            w.emit(f"const {var.name} = await retry({options}, () => tools.items.get({{ id: {identifier} }}));")
        case "wait":
            var = w.declare("job", "int")
            every, within = r.choice(("100", "250")), r.choice(("600", "1000", "1000", "1000", "150"))
            poll = '() => tools.jobs.status({ id: "j1" })'
            w.emit(
                f"const {var.name}raw = await waitUntil({{ everyMs: {every}, withinMs: {within} }}, "
                f'{poll}, (j) => j.state === "done");'
            )
            w.emit(f"const {var.name} = {var.name}raw.progress;")
        case "loop":
            var = w.declare("paged", "strs")
            limit = r.choice(("2", "4", "10"))
            w.emit(
                f"const {var.name} = await loop({{ max: {limit} }}, {{ cursor: null, seen: [] }}, "
                "async ({ cursor, seen }) => {"
            )
            w.emit("  const page = await tools.pages.fetch({ cursor });")
            w.emit("  const all = seen.concat(page.items);")
            w.emit("  if (page.next == null) { return done(all); }")
            w.emit("  return next({ cursor: page.next, seen: all });")
            w.emit("});")
        case "parmap":
            var = w.declare("pm", "ints")
            ids = w.vars_of("strs")
            source = r.choice(ids).name if ids else "[\"i0\", \"i1\", \"i2\"]"
            limit = r.choice(("1", "2", "8"))
            body = "async (id) => (await tools.items.get({ id })).qty"
            w.emit(f"const {var.name} = await parMap({{ limit: {limit} }}, {source}, {body});")
        case _:
            w.emit(f"await sleep({r.choice(('10', '250', '0', '1000'))});")


def result_expression(w: Writer) -> str:
    r = w.rng
    candidates = [v for scope in w.scopes for v in scope if v.type != "items" or r.random() < 0.5]
    chosen = r.sample(candidates, min(len(candidates), r.randint(1, 4)))
    if not chosen:
        return r.choice(("null", "1", "\"done\""))
    if r.random() < 0.3 and len(chosen) == 1:
        return chosen[0].name
    return "{ " + ", ".join(f"{v.name}: {v.name}" if r.random() < 0.7 else v.name for v in chosen) + " }"


def write_program(seed: int) -> str:
    w = Writer(random.Random(seed))
    w.helpers = []
    statements(w, w.rng.randint(4, 12), 2)
    if w.rng.random() < 0.08:
        ints = w.list_name("ints") if w.vars_of("ints") else "[1, 2]"
        strs = w.list_name("strs") if w.vars_of("strs") else '["a"]'
        w.emit(w.rng.choice(TRAP_STATEMENTS).replace("ints", ints).replace("strs", strs))
    w.emit(f"return {result_expression(w)};")
    return "\n".join([*w.helpers, *w.lines]) + "\n"
