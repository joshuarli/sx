from __future__ import annotations

import itertools
from collections.abc import Generator, Mapping, Sequence
from contextlib import contextmanager
from dataclasses import dataclass, field
from typing import TypeIs

from sx import ir
from sx.diagnostics import Diagnostic
from sx.registry import Capability, Effect, Function, Registry
from sx.syntax import (
    BoolLiteral,
    DurationLiteral,
    Form,
    IntLiteral,
    Keyword,
    ListLiteral,
    Node,
    Position,
    RecordLiteral,
    Span,
    StringLiteral,
    Symbol,
    show,
)
from sx.types import (
    BOOL,
    CALL_ERROR,
    CALL_ERROR_KIND,
    CALL_ERROR_KINDS,
    INT,
    NEVER,
    STRING,
    TRANSIENT_KINDS,
    EnumType,
    ListType,
    Never,
    NominalType,
    OptionType,
    Primitive,
    RecordType,
    ResultType,
    TupleType,
    Type,
    Variable,
    encodable_problem,
    show_type,
)
from sx.values import NONE_VALUE, UNIT_VALUE, VariantValue

SPECIAL_FORMS = frozenset({
    "let", "if", "match", "fn", "get", "call", "attempt", "retry", "wait-until", "loop", "recur", "par", "par-map",
    "some", "ok", "err",
})
HIGHER_ORDER = frozenset({"map", "filter", "keep", "any?", "all?", "reduce", "sort-by", "distinct-by", "group-by"})
PRELUDE = frozenset({
    "+", "-", "*", "quot", "rem", "=", "not=", "<", "<=", ">", ">=", "not", "and", "or", "str", "count", "first",
    "rest", "nth", "take", "concat", "flatten", "contains?",
}) | HIGHER_ORDER
FIXED_ARITY = {
    "+": 2, "*": 2, "quot": 2, "rem": 2, "=": 2, "not=": 2, "<": 2, "<=": 2, ">": 2, ">=": 2, "not": 1, "and": 2,
    "or": 2, "count": 1, "first": 1, "rest": 1, "nth": 2, "take": 2, "concat": 2, "flatten": 1, "contains?": 2,
    "map": 2, "filter": 2, "keep": 2, "any?": 2, "all?": 2, "reduce": 3, "sort-by": 2, "distinct-by": 2,
    "group-by": 2,
}
HABIT_HEADS = {
    "defn": ("defn", "Sx has no top-level definitions. Bind a local function with let: (let [f (fn [x] ...)] ...)."),
    "defn-": ("defn", "Sx has no top-level definitions. Bind a local function with let: (let [f (fn [x] ...)] ...)."),
    "def": ("defn", "Sx has no top-level definitions. Bind values with let."),
    "while": ("while", "Sx has no unbounded loops. Map over a list, or use loop with :max."),
    "for": ("while", "Sx has no for. Map over a list, or use loop with :max."),
    "doseq": ("while", "Sx has no doseq. Map over a list, and bind the result to _ if you only need the effects."),
    "dotimes": ("while", "Sx has no dotimes. Use loop with :max."),
    "assoc": ("assoc", "Sx has no assoc. Build a new record literal: {:a (get r :a) :b value}."),
    "assoc-in": ("assoc", "Sx has no assoc-in. Build a new record literal."),
    "update": ("assoc", "Sx has no update. Build a new record literal."),
    "merge": ("assoc", "Sx has no merge. Build a new record literal."),
    "dissoc": ("assoc", "Sx has no dissoc. Build a new record literal without the field."),
}
MISSING_FUNCTIONS = {
    "inc": "Add 1 instead: (+ x 1).",
    "dec": "Subtract 1 instead: (- x 1).",
    "last": "Use (nth xs (- (count xs) 1)).",
    "cond": "Nest if forms, or match on a value.",
    "when": "Use if with an else value.",
    "do": "Bind each step with let, and use _ for steps you run only for their effect.",
    "empty?": "Use (= (count xs) 0).",
    "nil?": "Match on the optional value: (match x (some v) ... none ...).",
    "some?": "Match on the optional value: (match x (some v) ... none ...).",
    "into": "Use concat to join two lists.",
    "conj": "Use concat with a one-element list: (concat xs [x]).",
    "max": "Use reduce: (reduce (fn [m x] (if (> x m) x m)) (first xs) (rest xs)).",
    "min": "Use reduce: (reduce (fn [m x] (if (< x m) x m)) (first xs) (rest xs)).",
    "apply": "Call the function with its arguments directly.",
    "println": "A program returns its result. It does not print.",
    "print": "A program returns its result. It does not print.",
    "keys": "Sx records have fixed fields. Read one with get.",
    "vals": "Sx records have fixed fields. Read one with get.",
    "remove": "Use filter with the opposite test.",
    "sort": "Use sort-by with the value itself as the key: (sort-by (fn [x] x) xs).",
}
DEFAULT_BINDER = "#present"
TEST_SUBJECT = {"if": "An if condition", "cond": "A cond test"}
OPTIONAL_HINT = "This value is optional. Take it apart first: (match x (some v) ... none ...)."
BINARY_SIGNATURE = {"+": "Int Int", "*": "Int Int", "quot": "Int Int", "rem": "Int Int"}


@dataclass(frozen=True, slots=True)
class Budgets:
    source_bytes: int = 65_536
    nodes: int = 20_000
    depth: int = 128
    steps: int = 2_000_000
    calls: int = 1_000
    concurrency: int = 16
    value_size: int = 1_000_000
    applications: int = 200_000
    delay_ms: int = 3_600_000
    instantiations: int = 2_000


DEFAULT_BUDGETS = Budgets()


@dataclass(frozen=True, slots=True)
class CheckResult:
    program: ir.Program | None
    diagnostics: tuple[Diagnostic, ...]


@dataclass(frozen=True, slots=True)
class ValueBinding:
    type: Type


@dataclass(frozen=True, slots=True)
class FunctionBinding:
    form: Form
    scope: Mapping[str, Binding]
    name: str


type Binding = ValueBinding | FunctionBinding


@dataclass(frozen=True, slots=True)
class Test:
    condition: ir.Node
    presence: bool
    name: str | None
    inner: Type | None


@dataclass(slots=True)
class LoopContext:
    types: list[Type]
    count: int


@dataclass(slots=True)
class Effects:
    calls: list[tuple[Capability, Span]] = field(default_factory=list)


def article(text: str) -> str:
    return f"an {text}" if text[:1] in "AEIOU" else f"a {text}"


def describe(t: Type) -> str:
    if isinstance(t, RecordType) and t != CALL_ERROR:
        return "a record"
    return article(show_type(t))


def edit_distance(a: str, b: str) -> int:
    previous: list[int] = list(range(len(b) + 1))
    for i, ca in enumerate(a, 1):
        current: list[int] = [i]
        for j, cb in enumerate(b, 1):
            current.append(min(previous[j] + 1, current[j - 1] + 1, previous[j - 1] + (ca != cb)))
        previous = current
    return previous[-1]


def closest(name: str, candidates: Sequence[str]) -> str | None:
    scored = sorted((edit_distance(name, c), c) for c in candidates if c != name)
    return scored[0][1] if scored and scored[0][0] <= 2 else None


def head_name(node: Node) -> str | None:
    if isinstance(node, Form) and node.items and isinstance(node.items[0], Symbol):
        return node.items[0].name
    return None


def is_function_form(node: Node) -> TypeIs[Form]:
    return head_name(node) == "fn"


def pattern_names(node: Node) -> list[str]:
    match node:
        case Symbol(name=name) if name[:1].islower() and name != "none":
            return [name]
        case Form(items=items) | ListLiteral(items=items):
            return [name for item in items[1:] if isinstance(node, Form) for name in pattern_names(item)] + [
                name for item in items if isinstance(node, ListLiteral) for name in pattern_names(item)
            ]
        case _:
            return []


def check(
    program: Node,
    registry: Registry,
    *,
    allowed: frozenset[str],
    inputs: Mapping[str, Type] | None = None,
    budgets: Budgets = DEFAULT_BUDGETS,
) -> CheckResult:
    checker = _Checker(registry, allowed, budgets)
    declared_inputs = dict(inputs or {})
    scope: dict[str, Binding] = {name: ValueBinding(t) for name, t in declared_inputs.items()}
    body, result = checker.check(program, None, scope)
    checker.check_size(program)
    final = checker.zonk(result)
    problem = encodable_problem(final)
    if problem is not None:
        checker.error(
            "no-encoding", program.span,
            f"The program's result is {describe(final)}, which {problem[len(show_type(final)) + 1:]}.",
        )
    diagnostics = tuple(sorted(checker.diagnostics, key=lambda d: (d.span.start.line, d.span.start.column)))
    if diagnostics:
        return CheckResult(None, diagnostics)
    used_inputs = frozenset(name for name in declared_inputs if name in checker.inputs_used)
    return CheckResult(ir.Program(body, final, frozenset(checker.capabilities_used), used_inputs), ())


class _Checker:
    def __init__(self, registry: Registry, allowed: frozenset[str], budgets: Budgets) -> None:
        self.registry = registry
        self.allowed = allowed
        self.budgets = budgets
        self.substitution: dict[int, Type] = {}
        self.counter = itertools.count()
        self.diagnostics: list[Diagnostic] = []
        self.effects: list[Effects] = []
        self.capabilities_used: set[str] = set()
        self.inputs_used: set[str] = set()
        self.tail: LoopContext | None = None
        self.instantiating: list[str] = []
        self.instantiations = 0
        self.sort_key_owner: Form | None = None

    def error(self, code: str, span: Span, message: str, **extra: str | None) -> None:
        self.diagnostics.append(Diagnostic(code=code, span=span, message=message, **extra))

    def habit(self, construct: str, span: Span, message: str, **extra: str | None) -> None:
        self.diagnostics.append(Diagnostic(code="habit", construct=construct, span=span, message=message, **extra))

    def placeholder(self, span: Span) -> tuple[ir.Node, Type]:
        return ir.Literal(UNIT_VALUE, span), NEVER

    def check_size(self, program: Node) -> None:
        nodes = 0
        stack: list[tuple[Node, int]] = [(program, 1)]
        deepest = 0
        while stack:
            node, depth = stack.pop()
            nodes += 1
            deepest = max(deepest, depth)
            if isinstance(node, Form | ListLiteral):
                stack.extend((child, depth + 1) for child in node.items)
            elif isinstance(node, RecordLiteral):
                stack.extend((f.value, depth + 1) for f in node.fields)
        if nodes > self.budgets.nodes:
            self.error(
                "too-large", program.span,
                f"The program has {nodes} syntax nodes, more than the nodes budget of {self.budgets.nodes}.",
            )
        if deepest > self.budgets.depth:
            self.error(
                "too-large", program.span,
                f"The program nests {deepest} levels deep, more than the depth budget of {self.budgets.depth}.",
            )


    def fresh(self) -> Variable:
        return Variable(next(self.counter))

    def resolve(self, t: Type) -> Type:
        while isinstance(t, Variable) and t.id in self.substitution:
            t = self.substitution[t.id]
        return t

    def zonk(self, t: Type) -> Type:
        t = self.resolve(t)
        match t:
            case ListType(element=element):
                return ListType(self.zonk(element))
            case TupleType(items=items):
                return TupleType(tuple(self.zonk(item) for item in items))
            case OptionType(inner=inner):
                return OptionType(self.zonk(inner))
            case ResultType(ok=ok, err=err):
                return ResultType(self.zonk(ok), self.zonk(err))
            case RecordType(fields=fields):
                return RecordType(tuple((name, self.zonk(ft)) for name, ft in fields), t.name)
            case _:
                return t

    def occurs(self, variable: Variable, t: Type) -> bool:
        t = self.resolve(t)
        match t:
            case Variable(id=other):
                return other == variable.id
            case ListType(element=element):
                return self.occurs(variable, element)
            case OptionType(inner=inner):
                return self.occurs(variable, inner)
            case TupleType(items=items):
                return any(self.occurs(variable, item) for item in items)
            case ResultType(ok=ok, err=err):
                return self.occurs(variable, ok) or self.occurs(variable, err)
            case RecordType(fields=fields):
                return any(self.occurs(variable, ft) for _, ft in fields)
            case _:
                return False

    def unify(self, a: Type, b: Type) -> bool:
        a, b = self.resolve(a), self.resolve(b)
        if a == b or isinstance(a, Never) or isinstance(b, Never):
            return True
        if isinstance(a, Variable):
            if self.occurs(a, b):
                return False
            self.substitution[a.id] = b
            return True
        if isinstance(b, Variable):
            return self.unify(b, a)
        match a, b:
            case ListType(), ListType():
                return self.unify(a.element, b.element)
            case OptionType(), OptionType():
                return self.unify(a.inner, b.inner)
            case TupleType(), TupleType():
                return len(a.items) == len(b.items) and all(
                    self.unify(x, y) for x, y in zip(a.items, b.items, strict=True)
                )
            case ResultType(), ResultType():
                return self.unify(a.ok, b.ok) and self.unify(a.err, b.err)
            case RecordType(), RecordType():
                if a.names != b.names:
                    return False
                return all(self.unify(ft, b.field(name) or NEVER) for name, ft in a.fields)
            case _:
                return False

    def trial(self, a: Type, b: Type) -> bool:
        snapshot = dict(self.substitution)
        if self.unify(a, b):
            return True
        self.substitution = snapshot
        return False

    def convert(self, actual: Type, expected: Type) -> tuple[bool, ir.Conversion | None]:
        snapshot = dict(self.substitution)
        if self.unify(actual, expected):
            return True, None
        self.substitution = snapshot
        a, e = self.resolve(actual), self.resolve(expected)
        if isinstance(e, OptionType) and not isinstance(a, OptionType | Variable | Never):
            ok, inner = self.convert(a, e.inner)
            if ok:
                return True, ir.ToSome(inner)
        elif isinstance(a, RecordType) and isinstance(e, RecordType) and a.names == e.names:
            conversions: list[tuple[str, ir.Conversion | None]] = []
            for name, ft in e.fields:
                ok, conversion = self.convert(a.field(name) or NEVER, ft)
                if not ok:
                    break
                conversions.append((name, conversion))
            else:
                return True, ir.EachField(tuple(conversions))
        elif isinstance(a, ListType) and isinstance(e, ListType):
            ok, conversion = self.convert(a.element, e.element)
            if ok and conversion is not None:
                return True, ir.EachElement(conversion)
        elif isinstance(a, TupleType) and isinstance(e, TupleType) and len(a.items) == len(e.items):
            items = [self.convert(x, y) for x, y in zip(a.items, e.items, strict=True)]
            if all(ok for ok, _ in items):
                return True, ir.EachItem(tuple(conversion for _, conversion in items))
        self.substitution = snapshot
        return False, None

    def coerce(
        self, node: ir.Node, actual: Type, expected: Type | None, span: Span, message: str | None = None,
        detail_hint: str | None = None, help_name: str | None = None,
    ) -> tuple[ir.Node, Type]:
        if expected is None:
            return node, actual
        ok, conversion = self.convert(actual, expected)
        if ok:
            return (node if conversion is None else ir.Coerce(node, conversion, span)), expected
        found, wanted = self.zonk(actual), self.zonk(expected)
        detail = f"Expected: {show_type(wanted)}\nFound: {show_type(found)}"
        if detail_hint is None and isinstance(found, OptionType) and self.trial(found.inner, wanted):
            detail_hint = OPTIONAL_HINT
        if detail_hint is not None:
            detail += f"\n{detail_hint}"
        self.error(
            "type-mismatch", span,
            message or f"This is {describe(found)}, but {describe(wanted)} is needed here.",
            detail=detail, help=help_name,
        )
        return node, expected

    def join(self, branches: list[tuple[ir.Node, Type, Span]], expected: Type | None) -> tuple[list[ir.Node], Type]:
        if expected is not None:
            return [self.coerce(node, t, expected, span)[0] for node, t, span in branches], expected
        target: Type = NEVER
        for _, t, span in branches:
            if isinstance(self.resolve(t), Never):
                continue
            if isinstance(self.resolve(target), Never):
                target = t
                continue
            snapshot = dict(self.substitution)
            if self.convert(t, target)[0]:
                self.substitution = snapshot
                continue
            self.substitution = snapshot
            if self.convert(target, t)[0]:
                self.substitution = snapshot
                target = t
                continue
            self.substitution = snapshot
            self.error(
                "branch-mismatch", span,
                f"The branches return different types: {show_type(self.zonk(target))} and {show_type(self.zonk(t))}.",
                detail="Every branch of an if or match must return the same type.",
            )
            return [node for node, _, _ in branches], target
        return [self.coerce(node, t, target, span)[0] for node, t, span in branches], target

    def is_sort_key(self, t: Type) -> bool:
        t = self.resolve(t)
        match t:
            case Primitive():
                return t in {INT, STRING, BOOL}
            case TupleType(items=items):
                return all(self.is_sort_key(item) for item in items)
            case ListType(element=element):
                return self.is_sort_key(element)
            case NominalType(base=base):
                return self.is_sort_key(base)
            case Never() | Variable():
                return True
            case _:
                return False


    @contextmanager
    def collecting(self) -> Generator[Effects]:
        effects = Effects()
        self.effects.append(effects)
        try:
            yield effects
        finally:
            self.effects.pop()

    @contextmanager
    def outside_tail(self) -> Generator[None]:
        saved, self.tail = self.tail, None
        try:
            yield
        finally:
            self.tail = saved


    def check(self, node: Node, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        match node:
            case IntLiteral(value=value, span=span):
                return self.coerce(ir.Literal(value, span), INT, expected, span)
            case StringLiteral(value=value, span=span):
                return self.coerce(ir.Literal(value, span), STRING, expected, span)
            case BoolLiteral(value=value, span=span):
                return self.coerce(ir.Literal(value, span), BOOL, expected, span)
            case DurationLiteral(span=span):
                self.error("bad-literal", span, "A duration only appears in retry and wait-until options.")
                return self.placeholder(span)
            case Keyword(name=name, span=span):
                self.habit(
                    "keyword-value", span, "Keywords only name record fields. Use a string for a label.",
                    fix=f'"{name}"',
                )
                return self.placeholder(span)
            case Symbol():
                return self.symbol(node, expected, scope)
            case ListLiteral():
                with self.outside_tail():
                    return self.bracket(node, expected, scope)
            case RecordLiteral():
                with self.outside_tail():
                    return self.record(node, expected, scope)
            case Form():
                return self.form(node, expected, scope)

    def peel(self, expected: Type | None) -> Type | None:
        if expected is None:
            return None
        resolved = self.resolve(expected)
        return resolved.inner if isinstance(resolved, OptionType) else expected

    def symbol(self, node: Symbol, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        name, span = node.name, node.span
        binding = scope.get(name)
        if isinstance(binding, ValueBinding):
            self.inputs_used.add(name)
            return self.coerce(ir.Variable(name, span), binding.type, expected, span)
        if isinstance(binding, FunctionBinding):
            self.error(
                "fn-position", span,
                (
                    f"{name} is a local function. Call it, (({name} ...)), or pass it to map, filter, and the like."
                ).replace("((", "(").replace("))", ")"),
            )
            return self.placeholder(span)
        if name == "none":
            inner = self.peel(expected)
            t = OptionType(inner if inner is not None else self.fresh())
            return self.coerce(ir.Literal(NONE_VALUE, span), t, expected, span)
        if name == "_":
            self.error("bad-pattern", span, "_ discards a value in a pattern. It is not a value itself.")
            return self.placeholder(span)
        if name[0].isupper():
            return self.variant(node, expected)
        if name in PRELUDE or self.registry.function(name) is not None:
            self.error("fn-position", span, f"{name} is a function. Call it, or pass it to map, filter, and the like.")
            return self.placeholder(span)
        if self.registry.capability(name) is not None:
            self.error("fn-position", span, f"{name} is a tool. Call it with call: (call {name} {{...}}).")
            return self.placeholder(span)
        suggestion = closest(name, [n for n, b in scope.items() if isinstance(b, ValueBinding)])
        self.error(
            "unknown-name", span, f"{name} is not defined here.",
            fix=suggestion,
        )
        return self.placeholder(span)

    def enums(self) -> list[EnumType]:
        found = [t for t in self.registry.types.values() if isinstance(t, EnumType)]
        return [CALL_ERROR_KIND, *found]

    def variant(self, node: Symbol, expected: Type | None) -> tuple[ir.Node, Type]:
        name, span = node.name, node.span
        hint = self.resolve(self.peel(expected) or NEVER)
        candidates = (
            [hint] if isinstance(hint, EnumType) and hint.has(name) else [e for e in self.enums() if e.has(name)]
        )
        if not candidates:
            self.error("unknown-variant", span, f"{name} is not a variant of any enum in scope.")
            return self.placeholder(span)
        if len(candidates) > 1:
            names = ", ".join(e.name for e in candidates)
            self.error(
                "unresolved-type", span,
                f"{name} is a variant of several enums ({names}), and the checker cannot tell which.",
            )
            return self.placeholder(span)
        enum = candidates[0]
        if enum.payload(name) is not None:
            self.error(
                "unknown-variant", span,
                f"{name} carries a value, so only a core function constructs it. Match on it with ({name} x).",
            )
            return self.placeholder(span)
        return self.coerce(ir.Literal(VariantValue(enum.name, name), span), enum, expected, span)

    def bracket(self, node: ListLiteral, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        hint = self.resolve(self.peel(expected) or NEVER)
        if isinstance(hint, ListType):
            items = [self.check(item, hint.element, scope)[0] for item in node.items]
            return self.coerce(ir.MakeList(tuple(items), span), hint, expected, span)
        if isinstance(hint, TupleType) and len(hint.items) == len(node.items):
            items = [self.check(item, t, scope)[0] for item, t in zip(node.items, hint.items, strict=True)]
            return self.coerce(ir.MakeTuple(tuple(items), span), hint, expected, span)
        checked = [self.check(item, None, scope) for item in node.items]
        if not checked:
            return self.coerce(ir.MakeList((), span), ListType(self.fresh()), expected, span)
        element = checked[0][1]
        if all(self.trial(t, element) for _, t in checked[1:]):
            return self.coerce(ir.MakeList(tuple(n for n, _ in checked), span), ListType(element), expected, span)
        t = TupleType(tuple(t for _, t in checked))
        return self.coerce(ir.MakeTuple(tuple(n for n, _ in checked), span), t, expected, span)

    def record(
        self, node: RecordLiteral, expected: Type | None, scope: Mapping[str, Binding], owner: str | None = None,
    ) -> tuple[ir.Node, Type]:
        span = node.span
        hint = self.resolve(self.peel(expected) or NEVER)
        if isinstance(hint, RecordType):
            fields: list[tuple[str, ir.Node]] = []
            given = {f.key.name for f in node.fields}
            for f in node.fields:
                wanted = hint.field(f.key.name)
                if wanted is None:
                    names = ", ".join(f":{name}" for name, _ in hint.fields)
                    where = f"The input to {owner}" if owner is not None else "This record"
                    self.error(
                        "unknown-field", f.key.span, f"{where} has no field :{f.key.name}.",
                        detail=f"Fields: {names}",
                        fix=(lambda s: None if s is None else f":{s}")(
                            closest(f.key.name, [n for n, _ in hint.fields]),
                        ),
                    )
                    self.check(f.value, None, scope)
                    continue
                fields.append((f.key.name, self.check(f.value, wanted, scope)[0]))
            for name, _ in hint.fields:
                if name not in given:
                    where = f"The input to {owner}" if owner is not None else "This record"
                    self.error(
                        "missing-field", span, f"{where} is missing the field :{name}.",
                        detail=f"Expected: {show_type(self.zonk(hint))}",
                    )
            order = [name for name, _ in hint.fields]
            fields.sort(key=lambda item: order.index(item[0]))
            return self.coerce(ir.MakeRecord(tuple(fields), span), hint, expected, span)
        checked = [(f.key.name, *self.check(f.value, None, scope)) for f in node.fields]
        t = RecordType(tuple((name, ft) for name, _, ft in checked))
        return self.coerce(ir.MakeRecord(tuple((name, n) for name, n, _ in checked), span), t, expected, span)


    def form(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if not node.items:
            self.error("bad-literal", span, "() is not an expression. Sx has no empty form.")
            return self.placeholder(span)
        head = node.items[0]
        if isinstance(head, Keyword):
            target = show(node.items[1]) if len(node.items) > 1 else "r"
            self.habit(
                "keyword-call", head.span, "A keyword is not a function. Read a field with get.",
                fix=f"(get {target} :{head.name})", help="get",
            )
            for item in node.items[1:]:
                self.check(item, None, scope)
            return self.placeholder(span)
        if not isinstance(head, Symbol):
            self.error("bad-literal", head.span, "A form starts with the name of a form or function.")
            return self.placeholder(span)
        name = head.name
        if name in {"let", "if", "match", "loop"}:
            return self.special_form(name, node, expected, scope)
        if name == "recur":
            return self.form_recur(node, scope)
        with self.outside_tail():
            binding = scope.get(name)
            if isinstance(binding, FunctionBinding):
                return self.apply_local(node, binding, expected, scope)
            if isinstance(binding, ValueBinding):
                self.error(
                    "type-mismatch", head.span, f"{name} is {describe(self.zonk(binding.type))}, not a function.",
                )
                return self.placeholder(span)
            if name in SPECIAL_FORMS:
                return self.special_form(name, node, expected, scope)
            if name in PRELUDE:
                return self.prelude(node, expected, scope)
            function = self.registry.function(name)
            if function is not None:
                return self.core(node, function, expected, scope)
            return self.unknown_head(node, scope)

    def unknown_head(self, node: Form, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        head = node.items[0]
        assert isinstance(head, Symbol)
        name = head.name
        construct = HABIT_HEADS[name][0] if name in HABIT_HEADS else None
        for item in node.items[1:]:
            if construct == "defn" or (construct == "assoc" and isinstance(item, Keyword)):
                continue
            with self.collecting():
                self.check(item, None, scope)
        if name in HABIT_HEADS:
            construct, message = HABIT_HEADS[name]
            self.habit(construct, head.span, message, help="let" if construct == "defn" else None)
        elif self.registry.capability(name) is not None:
            rest = " ".join(show(item) for item in node.items[1:])
            self.error(
                "unknown-function", head.span, f"{name} is a tool, so call it with call.",
                fix=f"(call {name} {rest})",
            )
        elif name in MISSING_FUNCTIONS:
            self.error("unknown-function", head.span, f"Sx has no {name}.", detail=MISSING_FUNCTIONS[name])
        else:
            candidates = [
                *PRELUDE, *self.registry.functions, *(n for n, b in scope.items() if isinstance(b, FunctionBinding)),
            ]
            suggestion = closest(name, candidates)
            self.error("unknown-function", head.span, f"{name} is not a function or form Sx knows.", fix=suggestion)
        return self.placeholder(node.span)

    def arity(self, node: Form, low: int, high: int | None, signature: str) -> bool:
        given = len(node.items) - 1
        if given >= low and (high is None or given <= high):
            return True
        name = node.items[0].name if isinstance(node.items[0], Symbol) else "?"
        wanted = str(low) if high == low else (f"at least {low}" if high is None else f"{low} or {high}")
        self.error(
            "arity", node.span,
            f"{name} takes {wanted} argument{'s' if (high or low) != 1 else ''}, but this call has {given}.",
            detail=f"Signature: ({name} {signature})".rstrip(), help=name,
        )
        return False

    def special_form(
        self, name: str, node: Form, expected: Type | None, scope: Mapping[str, Binding],
    ) -> tuple[ir.Node, Type]:
        match name:
            case "let":
                return self.form_let(node, expected, scope)
            case "if":
                return self.form_if(node, expected, scope)
            case "match":
                return self.form_match(node, expected, scope)
            case "loop":
                return self.form_loop(node, expected, scope)
            case "fn":
                return self.form_fn(node, expected, scope)
            case "get":
                return self.form_get(node, expected, scope)
            case "call":
                return self.form_call(node, expected, scope)
            case "attempt":
                return self.form_attempt(node, expected, scope)
            case "retry":
                return self.form_retry(node, expected, scope)
            case "wait-until":
                return self.form_wait_until(node, expected, scope)
            case "par":
                return self.form_par(node, expected, scope)
            case "par-map":
                return self.form_par_map(node, expected, scope)
            case "some":
                return self.form_some(node, expected, scope)
            case "ok":
                return self.form_ok(node, expected, scope)
            case "err":
                return self.form_err(node, expected, scope)
        raise AssertionError(name)

    def form_let(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if len(node.items) < 2 or not isinstance(node.items[1], ListLiteral):
            self.error("bad-pattern", span, "let needs a binding vector: (let [name value] body).", help="let")
            return self.placeholder(span)
        bindings = node.items[1].items
        if len(bindings) % 2:
            self.error(
                "bad-pattern", node.items[1].span,
                "A let binding vector pairs each name with a value, but this one has an odd count.", help="let",
            )
            return self.placeholder(span)
        inner: dict[str, Binding] = dict(scope)
        steps: list[tuple[str, ir.Binder | str, ir.Node | None, Span]] = []
        for pattern, value in zip(bindings[::2], bindings[1::2], strict=True):
            if is_function_form(value):
                if not isinstance(pattern, Symbol) or pattern.name == "_":
                    self.error("bad-pattern", pattern.span, "A local function needs a name.")
                    continue
                self.check_fn_shape(value)
                function_binding: Binding = FunctionBinding(value, dict(inner), pattern.name)
                inner = dict(inner)
                inner[pattern.name] = function_binding
                steps.append(("function", pattern.name, None, pattern.span))
                continue
            with self.outside_tail():
                value_ir, value_type = self.check(value, None, inner)
            binder, inner = self.bind(pattern, value_type, inner, role="let")
            steps.append(("value", binder, value_ir, pattern.span))
        body_nodes = node.items[2:]
        if not body_nodes:
            self.error("arity", span, "A let needs a body expression after its bindings.", help="let")
            return self.placeholder(span)
        if len(body_nodes) > 1:
            self.habit(
                "let-body", body_nodes[1].span, f"A let has one body expression, but this one has {len(body_nodes)}.",
                detail="Bind a step that runs for its effect to _ among the bindings: [_ (map ...)].", help="let",
            )
            for extra in body_nodes[:-1]:
                with self.outside_tail():
                    self.check(extra, None, inner)
        body, body_type = self.check(body_nodes[-1], expected, inner)
        for kind, binder, value_ir, step_span in reversed(steps):
            if kind == "function":
                assert isinstance(binder, str)
                body = ir.LocalFunction(binder, body, step_span)
            else:
                assert value_ir is not None
                assert not isinstance(binder, str)
                body = ir.Let(binder, value_ir, body, step_span)
        return body, body_type

    def check_fn_shape(self, node: Node) -> bool:
        assert isinstance(node, Form)
        if len(node.items) < 3 or not isinstance(node.items[1], ListLiteral):
            self.error("bad-pattern", node.span, "fn needs a parameter vector and a body: (fn [x] body).", help="fn")
            return False
        if len(node.items) > 3:
            self.error("arity", node.items[3].span, "A fn has one body expression.", help="fn")
            return False
        return True

    def bind(
        self, pattern: Node, t: Type, scope: Mapping[str, Binding], role: str, fix: str | None = None
    ) -> tuple[ir.Binder, dict[str, Binding]]:
        inner = dict(scope)
        match pattern:
            case Symbol(name="_"):
                return ir.Discard(), inner
            case Symbol(name=name, span=span) if not name[0].isupper() and name != "none":
                inner[name] = ValueBinding(t)
                return ir.BindName(name), inner
            case ListLiteral(items=items, span=span):
                resolved = self.resolve(t)
                if isinstance(resolved, Variable):
                    resolved = TupleType(tuple(self.fresh() for _ in items))
                    self.unify(t, resolved)
                if isinstance(resolved, Never):
                    resolved = TupleType(tuple(NEVER for _ in items))
                if isinstance(resolved, RecordType):
                    message = (
                        f"This parameter takes a tuple apart, but {role} passes it a record."
                        if role not in {"let", "loop"}
                        else "This pattern takes a tuple apart, but the value is a record."
                    )
                    self.error(
                        "type-mismatch", span, message,
                        detail=f"Expected: {show_type(self.zonk(resolved))}\nFound: a tuple pattern {show(pattern)}",
                        fix=fix,
                        help=role if role not in {"let", "loop"} else None,
                    )
                    for item in items:
                        _, inner = self.bind(item, NEVER, inner, role)
                    return ir.Discard(), inner
                if not isinstance(resolved, TupleType) or len(resolved.items) != len(items):
                    self.error(
                        "type-mismatch", span,
                        f"This pattern takes apart a tuple of {len(items)}, "
                        f"but the value is {describe(self.zonk(resolved))}.",
                        detail=f"Found: {show_type(self.zonk(resolved))}",
                    )
                    for item in items:
                        _, inner = self.bind(item, NEVER, inner, role)
                    return ir.Discard(), inner
                binders = []
                for item, item_type in zip(items, resolved.items, strict=True):
                    binder, inner = self.bind(item, item_type, inner, role)
                    binders.append(binder)
                return ir.BindTuple(tuple(binders)), inner
            case _:
                constructor = head_name(pattern)
                detail = None
                if constructor in {"some", "ok", "err"} or (constructor is not None and constructor[:1].isupper()):
                    detail = f"To take a value apart, use match: (match value {show(pattern)} ... _ ...)."
                self.error(
                    "bad-pattern", pattern.span,
                    f"{show(pattern)} is not a pattern. A pattern is a name, _, or [a b ...].",
                    detail=detail,
                )
                for name in pattern_names(pattern):
                    inner[name] = ValueBinding(NEVER)
                return ir.Discard(), inner

    def check_test(self, condition_node: Node, scope: Mapping[str, Binding], who: str) -> Test:
        with self.outside_tail():
            condition, condition_type = self.check(condition_node, None, scope)
        resolved = self.resolve(condition_type)
        if isinstance(resolved, OptionType):
            inner = self.resolve(resolved.inner)
            if inner == BOOL:
                self.error(
                    "condition-not-bool", condition_node.span,
                    f"{TEST_SUBJECT[who]} must be true or false, "
                    f"but this is {describe(self.zonk(condition_type))}.",
                    detail=(
                        "Presence and truth would disagree: (some false) is present but false.\n"
                        f"Match on it instead: (match {show(condition_node)} (some v) ... none ...)."
                    ),
                    help=who,
                )
                return Test(condition, False, None, None)
            if not isinstance(inner, Variable | Never):
                name = condition_node.name if isinstance(condition_node, Symbol) else None
                narrows = name is not None and isinstance(scope.get(name), ValueBinding)
                return Test(condition, True, name if narrows else None, resolved.inner)
        if not self.trial(condition_type, BOOL):
            hint = (
                "To test an optional value, match on it: (match x (some v) ... none ...)."
                if isinstance(resolved, OptionType)
                else None
            )
            self.error(
                "condition-not-bool", condition_node.span,
                f"{TEST_SUBJECT[who]} must be true or false, "
                f"but this is {describe(self.zonk(condition_type))}.",
                detail=hint, help=who,
            )
        return Test(condition, False, None, None)

    def narrowed(self, test: Test, scope: Mapping[str, Binding]) -> Mapping[str, Binding]:
        if test.name is None or test.inner is None:
            return scope
        return {**scope, test.name: ValueBinding(test.inner)}

    def lower_test(self, test: Test, then: ir.Node, otherwise: ir.Node, span: Span) -> ir.Node:
        if not test.presence:
            return ir.If(test.condition, then, otherwise, span)
        binder: ir.Binder = ir.BindName(test.name) if test.name is not None else ir.Discard()
        return ir.Match(
            test.condition, ((ir.MatchSome(ir.MatchAny(binder)), then), (ir.MatchNone(), otherwise)), span,
        )

    def form_if(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        if not self.arity(node, 3, 3, "condition then else"):
            return self.placeholder(node.span)
        _, condition_node, then_node, else_node = node.items
        test = self.check_test(condition_node, scope, "if")
        then, then_type = self.check(then_node, expected, self.narrowed(test, scope))
        otherwise, else_type = self.check(else_node, expected, scope)
        (then, otherwise), t = self.join(
            [(then, then_type, then_node.span), (otherwise, else_type, else_node.span)], expected,
        )
        return self.lower_test(test, then, otherwise, node.span), t

    def form_match(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if len(node.items) < 4:
            self.error("arity", span, "match takes a value and at least one pattern and result.", help="match")
            return self.placeholder(span)
        with self.outside_tail():
            scrutinee, scrutinee_type = self.check(node.items[1], None, scope)
        clause_nodes = node.items[2:]
        if len(clause_nodes) % 2:
            self.error(
                "bad-pattern", clause_nodes[-1].span,
                "This match pairs each pattern with a result, but the last pattern has none.", help="match",
            )
            return self.placeholder(span)
        covered: set[str] = set()
        complete = False
        clauses: list[tuple[ir.MatchPattern, ir.Node, Type, Span]] = []
        pairs = list(zip(clause_nodes[::2], clause_nodes[1::2], strict=True))
        scrutinee_option = self.resolve(scrutinee_type)
        scrutinee_name = (
            node.items[1].name
            if isinstance(node.items[1], Symbol)
            and isinstance(scope.get(node.items[1].name), ValueBinding)
            and isinstance(scrutinee_option, OptionType)
            else None
        )
        for index, (pattern_node, body_node) in enumerate(pairs):
            pattern, inner, keys, total = self.match_pattern(pattern_node, scrutinee_type, scope)
            last = index == len(pairs) - 1
            wildcard = isinstance(pattern, ir.MatchAny)
            if (
                scrutinee_name is not None
                and isinstance(scrutinee_option, OptionType)
                and last
                and "none" in covered
                and isinstance(pattern_node, Symbol)
                and pattern_node.name == "_"
            ):
                pattern = ir.MatchSome(ir.MatchAny(ir.BindName(scrutinee_name)))
                inner = {**scope, scrutinee_name: ValueBinding(scrutinee_option.inner)}
            if complete and not (last and wildcard):
                self.error(
                    "unreachable-clause", pattern_node.span,
                    "This clause can never match, because the clauses before it cover every case.",
                )
            elif keys and keys <= covered and not wildcard and total:
                self.error(
                    "unreachable-clause", pattern_node.span,
                    "This clause can never match, because an earlier clause covers the same case.",
                )
            if total:
                covered |= keys
            if wildcard:
                complete = True
            body, body_type = self.check(body_node, expected, inner)
            clauses.append((pattern, body, body_type, body_node.span))
        if not complete:
            missing = self.missing_cases(scrutinee_type, covered)
            if missing:
                self.error(
                    "non-exhaustive", span, f"This match does not handle {', '.join(missing)}.",
                    detail="Add a clause for each missing case, or end with _ to match anything else.", help="match",
                )
        bodies, t = self.join([(body, body_type, body_span) for _, body, body_type, body_span in clauses], expected)
        return ir.Match(
            scrutinee, tuple((pattern, body) for (pattern, _, _, _), body in zip(clauses, bodies, strict=True)), span,
        ), t

    def missing_cases(self, t: Type, covered: set[str]) -> list[str]:
        resolved = self.resolve(t)
        match resolved:
            case OptionType():
                return [case for case in ("(some x)", "none") if case.split(" ")[0].strip("(") not in covered]
            case ResultType():
                return [case for case in ("(ok x)", "(err e)") if case.split(" ")[0].strip("(") not in covered]
            case EnumType(variants=variants):
                return [name if payload is None else f"({name} x)" for name, payload in variants if name not in covered]
            case Never():
                return []
            case _:
                return ["every other value; end with _"]

    def match_pattern(
        self, node: Node, t: Type, scope: Mapping[str, Binding],
    ) -> tuple[ir.MatchPattern, dict[str, Binding], set[str], bool]:
        resolved = self.resolve(t)
        if isinstance(node, Symbol) and node.name == "_":
            return ir.MatchAny(ir.Discard()), dict(scope), set(), True
        if isinstance(node, Symbol) and node.name == "none":
            if isinstance(resolved, Variable):
                resolved = OptionType(self.fresh())
                self.unify(t, resolved)
            if not isinstance(resolved, OptionType | Never):
                return self.pattern_mismatch(node, t, scope)
            return ir.MatchNone(), dict(scope), {"none"}, True
        if isinstance(node, Symbol) and node.name[0].isupper():
            if isinstance(resolved, Never):
                return ir.MatchVariant(node.name, None), dict(scope), {node.name}, True
            if not isinstance(resolved, EnumType):
                return self.pattern_mismatch(node, t, scope)
            if not resolved.has(node.name):
                self.error(
                    "unknown-variant", node.span, f"{resolved.name} has no variant {node.name}.",
                    detail="Variants: " + ", ".join(v for v, _ in resolved.variants),
                )
                return ir.MatchVariant(node.name, None), dict(scope), set(), False
            if resolved.payload(node.name) is not None:
                self.error(
                    "bad-pattern", node.span, f"{node.name} carries a value, so match it as ({node.name} x).",
                    help="match",
                )
            return ir.MatchVariant(node.name, None), dict(scope), {node.name}, True
        if isinstance(node, Symbol):
            self.error(
                "bad-pattern", node.span, f"{node.name} is not a match pattern. Use _ to match anything.",
                help="match",
            )
            return ir.MatchAny(ir.Discard()), {**scope, node.name: ValueBinding(NEVER)}, set(), False
        if isinstance(node, Form) and len(node.items) == 2 and isinstance(node.items[0], Symbol):
            constructor, inner_node = node.items[0].name, node.items[1]
            if constructor in {"some", "ok", "err"}:
                if isinstance(resolved, Variable):
                    resolved = (
                        OptionType(self.fresh())
                        if constructor == "some"
                        else ResultType(self.fresh(), self.fresh())
                    )
                    self.unify(t, resolved)
                if isinstance(resolved, Never):
                    inner_type: Type = NEVER
                elif constructor == "some" and isinstance(resolved, OptionType):
                    inner_type = resolved.inner
                elif constructor == "ok" and isinstance(resolved, ResultType):
                    inner_type = resolved.ok
                elif constructor == "err" and isinstance(resolved, ResultType):
                    inner_type = resolved.err
                else:
                    return self.pattern_mismatch(node, t, scope)
                inner_pattern, inner_scope, total = self.sub_pattern(inner_node, inner_type, scope)
                wrapped = {"some": ir.MatchSome, "ok": ir.MatchOk, "err": ir.MatchErr}[constructor](inner_pattern)
                return wrapped, inner_scope, {constructor}, total
            if constructor[0].isupper():
                if isinstance(resolved, EnumType) and resolved.has(constructor):
                    payload = resolved.payload(constructor)
                    if payload is None:
                        self.error(
                            "bad-pattern", node.span,
                            f"{constructor} carries no value, so match it as {constructor}.", help="match",
                        )
                        return ir.MatchVariant(constructor, None), dict(scope), {constructor}, True
                    inner_pattern, inner_scope, total = self.sub_pattern(inner_node, payload, scope)
                    return ir.MatchVariant(constructor, inner_pattern), inner_scope, {constructor}, total
                if isinstance(resolved, EnumType):
                    self.error(
                        "unknown-variant", node.items[0].span,
                        f"{resolved.name} has no variant {constructor}.",
                        detail="Variants: " + ", ".join(v for v, _ in resolved.variants),
                    )
                    return ir.MatchVariant(constructor, None), dict(scope), set(), False
                return self.pattern_mismatch(node, t, scope)
        self.error(
            "bad-pattern", node.span, f"{show(node)} is not a match pattern.",
            detail="Patterns: (some x) none (ok x) (err e) Variant (Variant x) _", help="match",
        )
        return ir.MatchAny(ir.Discard()), dict(scope), set(), False

    def sub_pattern(
        self, node: Node, t: Type, scope: Mapping[str, Binding],
    ) -> tuple[ir.MatchPattern, dict[str, Binding], bool]:
        if (
            (isinstance(node, Symbol) and node.name[0].islower() and node.name != "none")
            or isinstance(node, ListLiteral)
            or (isinstance(node, Symbol) and node.name == "_")
        ):
            binder, inner = self.bind(node, t, scope, role="match")
            return ir.MatchAny(binder), inner, True
        pattern, inner, _, _ = self.match_pattern(node, t, scope)
        return pattern, inner, False

    def pattern_mismatch(
        self, node: Node, t: Type, scope: Mapping[str, Binding],
    ) -> tuple[ir.MatchPattern, dict[str, Binding], set[str], bool]:
        self.error(
            "bad-pattern", node.span, f"{show(node)} cannot match {describe(self.zonk(t))}.",
            detail=f"The value is {show_type(self.zonk(t))}.", help="match",
        )
        return ir.MatchAny(ir.Discard()), dict(scope), set(), False

    def form_fn(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        self.error(
            "fn-position", node.span,
            "A fn is only allowed as an argument to map, filter, and the like, or as the value of a let binding.",
            help="fn",
        )
        return self.placeholder(node.span)

    def form_get(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        if not self.arity(node, 2, 3, "record :field [default]"):
            return self.placeholder(node.span)
        record_node, key = node.items[1], node.items[2]
        default_node = node.items[3] if len(node.items) == 4 else None
        record_ir, record_type = self.check(record_node, None, scope)
        if not isinstance(key, Keyword):
            fix = f'(get {show(record_node)} :{key.value})' if isinstance(key, StringLiteral) else None
            self.error(
                "bad-literal", key.span, "get takes a field name as a keyword, such as :name.", fix=fix, help="get",
            )
            return self.placeholder(node.span)
        resolved = self.resolve(record_type)
        if isinstance(resolved, Never):
            return self.placeholder(node.span)
        if isinstance(resolved, Variable):
            self.error(
                "unresolved-type", record_node.span,
                "The checker cannot tell which record this is, so it cannot read a field from it.",
            )
            return self.placeholder(node.span)
        if not isinstance(resolved, RecordType):
            hint = (
                "Match on an optional value before reading its fields: (match x (some r) (get r :field) none ...)."
                if isinstance(resolved, OptionType)
                else None
            )
            self.error(
                "not-a-record", record_node.span,
                f"get reads a field from a record, but this is {describe(self.zonk(resolved))}.",
                detail=hint, help="get",
            )
            return self.placeholder(node.span)
        field_type = resolved.field(key.name)
        if field_type is None:
            names = [name for name, _ in resolved.fields]
            suggestion = closest(key.name, names)
            self.error(
                "unknown-field", key.span, f"This record has no field :{key.name}.",
                detail="Fields: " + ", ".join(f":{name}" for name in names),
                fix=None if suggestion is None else f"(get {show(record_node)} :{suggestion})", help="get",
            )
            return self.placeholder(node.span)
        getter = ir.Get(record_ir, key.name, node.span)
        if default_node is None:
            return self.coerce(getter, field_type, expected, node.span)
        resolved_field = self.resolve(field_type)
        if not isinstance(resolved_field, OptionType):
            self.error(
                "arity", default_node.span,
                f"The field :{key.name} is always present, so get takes no default for it.",
                detail=f"The field has type {show_type(self.zonk(field_type))}, not an Option.",
                fix=f"(get {show(record_node)} :{key.name})", help="get",
            )
            return self.coerce(getter, field_type, expected, node.span)
        default, _ = self.check(default_node, resolved_field.inner, scope)
        present = ir.Variable(DEFAULT_BINDER, node.span)
        choice = ir.Match(
            getter, ((ir.MatchSome(ir.MatchAny(ir.BindName(DEFAULT_BINDER))), present), (ir.MatchNone(), default)),
            node.span,
        )
        return self.coerce(choice, resolved_field.inner, expected, node.span)

    def form_call(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if len(node.items) < 2 or not isinstance(node.items[1], Symbol):
            self.error(
                "arity", span, "call takes a tool name and an input record: (call tool.name {...}).", help="call",
            )
            return self.placeholder(span)
        name_node = node.items[1]
        capability = self.registry.capability(name_node.name)
        if capability is None:
            suggestion = closest(name_node.name, list(self.registry.capabilities))
            self.error(
                "unknown-capability", name_node.span, f"There is no tool named {name_node.name}.", fix=suggestion,
            )
            for item in node.items[2:]:
                self.check(item, None, scope)
            return self.placeholder(span)
        if len(node.items) != 3:
            self.error(
                "arity", span,
                f"call takes a tool name and one input, but this call has {len(node.items) - 2} inputs.",
                detail=f"Input: {show_type(capability.input)}", fix=f"(call {capability.id} {{...}})",
                help=capability.id,
            )
            return self.placeholder(span)
        if capability.id not in self.allowed:
            allowed = ", ".join(sorted(self.allowed)) or "nothing"
            self.error(
                "unauthorized", name_node.span, f"This program may not call {capability.id}.",
                detail=f"The host allows: {allowed}",
            )
        argument_node = node.items[2]
        if isinstance(argument_node, RecordLiteral):
            argument, _ = self.record(argument_node, capability.input, scope, owner=capability.id)
        else:
            argument, _ = self.check(argument_node, capability.input, scope)
        for effects in self.effects:
            effects.calls.append((capability, span))
        self.capabilities_used.add(capability.id)
        return self.coerce(ir.Call(capability, argument, span), capability.output, expected, span)

    def form_some(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        if not self.arity(node, 1, 1, "value"):
            return self.placeholder(node.span)
        hint = self.resolve(expected) if expected is not None else None
        inner_expected = hint.inner if isinstance(hint, OptionType) else None
        value, t = self.check(node.items[1], inner_expected, scope)
        return self.coerce(ir.Wrap("some", value, node.span), OptionType(t), expected, node.span)

    def form_ok(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        return self.wrap_result(node, expected, scope, "ok")

    def form_err(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        return self.wrap_result(node, expected, scope, "err")

    def wrap_result(
        self, node: Form, expected: Type | None, scope: Mapping[str, Binding], kind: str,
    ) -> tuple[ir.Node, Type]:
        if not self.arity(node, 1, 1, "value"):
            return self.placeholder(node.span)
        hint = self.resolve(expected) if expected is not None else None
        result = hint if isinstance(hint, ResultType) else ResultType(self.fresh(), self.fresh())
        value, _ = self.check(node.items[1], result.ok if kind == "ok" else result.err, scope)
        return self.coerce(ir.Wrap(kind, value, node.span), result, expected, node.span)

    def form_attempt(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        if not self.arity(node, 1, 1, "expression"):
            return self.placeholder(node.span)
        hint = self.resolve(expected) if expected is not None else None
        body, t = self.check(node.items[1], hint.ok if isinstance(hint, ResultType) else None, scope)
        return self.coerce(ir.Attempt(body, node.span), ResultType(t, CALL_ERROR), expected, node.span)

    def options(self, node: Node, form: str, allowed: dict[str, str]) -> dict[str, Node] | None:
        if not isinstance(node, RecordLiteral):
            self.error(
                "bad-option", node.span,
                f"{form} takes its options as a record first, such as {next(iter(allowed.values()))}.", help=form,
            )
            return None
        found: dict[str, Node] = {}
        for f in node.fields:
            if f.key.name not in allowed:
                names = ", ".join(f":{name}" for name in allowed)
                self.error(
                    "bad-option", f.key.span, f"{form} has no option :{f.key.name}.",
                    detail=f"Options: {names}", help=form,
                )
                return None
            found[f.key.name] = f.value
        return found

    def durations(self, node: Node, form: str, option: str, count: int | None) -> list[int] | None:
        if isinstance(node, DurationLiteral) and count is None:
            return [node.milliseconds]
        if (
            isinstance(node, ListLiteral)
            and all(isinstance(i, DurationLiteral) for i in node.items)
            and (count is None or len(node.items) == count)
        ):
            return [item.milliseconds for item in node.items if isinstance(item, DurationLiteral)]
        shape = (
            "a list of durations such as [1s 2s 4s]"
            if count is None
            else f"a list of {count} durations such as [100ms 5s]"
        )
        self.error("bad-option", node.span, f":{option} takes {shape}.", help=form)
        return None

    def single_duration(self, node: Node, form: str, option: str) -> int | None:
        if isinstance(node, DurationLiteral):
            return node.milliseconds
        self.error("bad-option", node.span, f":{option} takes a duration such as 30s.", help=form)
        return None

    def backoff(self, first: int, cap: int, within: int) -> tuple[int, ...]:
        delays: list[int] = []
        delay, total = first, 0
        while total + delay <= within and len(delays) < 10_000:
            delays.append(delay)
            total += delay
            delay = min(delay * 2, cap)
        return tuple(delays)

    def form_retry(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if not self.arity(node, 2, 2, "{:delays [1s 2s]} expression"):
            return self.placeholder(span)
        options = self.options(
            node.items[1], "retry", {"delays": "{:delays [1s 2s 4s]}", "backoff": "", "within": "", "on": ""},
        )
        delays: tuple[int, ...] = ()
        kinds = TRANSIENT_KINDS
        if options is not None:
            if ("delays" in options) == ("backoff" in options):
                self.error(
                    "bad-option", node.items[1].span, "retry takes exactly one of :delays and :backoff.", help="retry",
                )
            elif "delays" in options:
                if "within" in options:
                    self.error(
                        "bad-option", node.items[1].span, ":within goes with :backoff, not :delays.", help="retry",
                    )
                found = self.durations(options["delays"], "retry", "delays", None)
                delays = tuple(found or ())
            else:
                pair = self.durations(options["backoff"], "retry", "backoff", 2)
                within = self.single_duration(options["within"], "retry", "within") if "within" in options else None
                if "within" not in options:
                    self.error(
                        "bad-option", node.items[1].span, ":backoff needs :within to bound the total delay.",
                        fix="{:backoff [100ms 5s] :within 30s}", help="retry",
                    )
                if pair is not None and within is not None:
                    if pair[0] < 1 or pair[1] < pair[0]:
                        self.error(
                            "bad-option", options["backoff"].span,
                            ":backoff takes a first delay of at least 1ms and a cap no smaller than it.", help="retry",
                        )
                    else:
                        delays = self.backoff(pair[0], pair[1], within)
            if "on" in options:
                kinds = self.kinds(options["on"]) or kinds
        with self.collecting() as effects:
            body, t = self.check(node.items[2], expected, scope)
        for capability, call_span in effects.calls:
            if capability.effect is Effect.WRITE and not capability.idempotent:
                self.error(
                    "retry-write", call_span,
                    f"retry may repeat {capability.id}, which is a write that is not idempotent.",
                    detail="Retry only reads and idempotent writes.", help="retry",
                )
        return ir.Retry(delays, frozenset(kinds), body, span), t

    def kinds(self, node: Node) -> frozenset[str] | None:
        if (
            isinstance(node, ListLiteral)
            and node.items
            and all(isinstance(i, Symbol) and i.name in CALL_ERROR_KINDS for i in node.items)
        ):
            return frozenset(i.name for i in node.items if isinstance(i, Symbol))
        self.error(
            "bad-option", node.span, ":on takes a list of error kinds, such as [Timeout Unavailable].",
            detail="Kinds: " + " ".join(CALL_ERROR_KINDS), help="retry",
        )
        return None

    def form_wait_until(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if not self.arity(node, 3, 3, "{:within 2m :every 5s} (fn [v] ...) expression"):
            return self.placeholder(span)
        options = self.options(
            node.items[1], "wait-until", {"within": "{:within 2m :every 5s}", "every": "", "backoff": ""},
        )
        intervals: tuple[int, ...] = ()
        if options is not None:
            within = self.single_duration(options["within"], "wait-until", "within") if "within" in options else None
            if "within" not in options:
                self.error(
                    "bad-option", node.items[1].span, "wait-until needs :within to bound the total wait.",
                    fix="{:within 2m :every 5s}", help="wait-until",
                )
            if ("every" in options) == ("backoff" in options):
                self.error(
                    "bad-option", node.items[1].span, "wait-until takes exactly one of :every and :backoff.",
                    help="wait-until",
                )
            elif within is not None and "every" in options:
                every = self.single_duration(options["every"], "wait-until", "every")
                if every is not None and every < 1:
                    self.error("bad-option", options["every"].span, ":every must be at least 1ms.", help="wait-until")
                elif every is not None:
                    intervals = tuple([every] * min(within // every, 10_000))
            elif within is not None:
                pair = self.durations(options["backoff"], "wait-until", "backoff", 2)
                if pair is not None and (pair[0] < 1 or pair[1] < pair[0]):
                    self.error(
                        "bad-option", options["backoff"].span,
                        ":backoff takes a first interval of at least 1ms and a cap no smaller than it.",
                        help="wait-until",
                    )
                elif pair is not None:
                    intervals = self.backoff(pair[0], pair[1], within)
        with self.collecting() as effects:
            body, t = self.check(node.items[3], expected, scope)
        for capability, call_span in effects.calls:
            if capability.effect is Effect.WRITE:
                self.error(
                    "wait-write", call_span,
                    f"wait-until repeats its expression, so it cannot call {capability.id}, which writes.",
                    help="wait-until",
                )
        predicate, _ = self.function_argument(node.items[2], [t], BOOL, "wait-until", scope)
        return ir.WaitUntil(intervals, predicate, body, span), t

    def form_loop(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if len(node.items) >= 2 and isinstance(node.items[1], ListLiteral):
            self.habit(
                "loop-without-max", node.items[1].span, "A loop needs a bound on its repetitions.",
                fix=f"(loop {{:max 100}} {show(node.items[1])} ...)", help="loop",
            )
            return self.placeholder(span)
        if not self.arity(node, 3, 3, "{:max 100} [name value ...] body"):
            return self.placeholder(span)
        options = self.options(node.items[1], "loop", {"max": "{:max 100}"})
        maximum = 1
        if options is not None:
            value = options.get("max")
            if not isinstance(value, IntLiteral) or value.value < 1:
                self.error(
                    "bad-option", node.items[1].span,
                    "loop takes {:max N}, where N is an integer literal of at least 1.", help="loop",
                )
            else:
                maximum = value.value
        bindings_node = node.items[2]
        if not isinstance(bindings_node, ListLiteral) or len(bindings_node.items) % 2:
            self.error(
                "bad-pattern", bindings_node.span,
                "loop needs a binding vector that pairs each name with a value.", help="loop",
            )
            return self.placeholder(span)
        inner: dict[str, Binding] = dict(scope)
        binders: list[ir.Binder] = []
        initial: list[ir.Node] = []
        types: list[Type] = []
        with self.outside_tail():
            for pattern, value in zip(bindings_node.items[::2], bindings_node.items[1::2], strict=True):
                value_ir, value_type = self.check(value, None, scope)
                binder, inner = self.bind(pattern, value_type, inner, role="loop")
                binders.append(binder)
                initial.append(value_ir)
                types.append(value_type)
        saved, self.tail = self.tail, LoopContext(types, len(types))
        try:
            body, t = self.check(node.items[3], expected, inner)
        finally:
            self.tail = saved
        return ir.Loop(maximum, tuple(binders), tuple(initial), body, span), t

    def form_recur(self, node: Form, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        context = self.tail
        span = node.span
        if context is None:
            self.error(
                "recur-position", span, "recur must be the last step of a loop body, and never inside a fn.",
                help="loop",
            )
            with self.outside_tail():
                for item in node.items[1:]:
                    self.check(item, None, scope)
            return self.placeholder(span)
        args = node.items[1:]
        if len(args) != context.count:
            self.error(
                "recur-arity", span,
                f"This loop binds {context.count} name{'s' if context.count != 1 else ''}, "
                f"so recur takes {context.count} value{'s' if context.count != 1 else ''}, "
                f"but this one has {len(args)}.",
                help="loop",
            )
            return self.placeholder(span)
        checked = []
        with self.outside_tail():
            for arg, t in zip(args, context.types, strict=True):
                checked.append(self.check(arg, t, scope)[0])
        return ir.Recur(tuple(checked), span), NEVER

    def form_par(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        if not self.arity(node, 1, None, "expression ..."):
            return self.placeholder(node.span)
        hint = self.resolve(expected) if expected is not None else None
        hints = (
            hint.items
            if isinstance(hint, TupleType) and len(hint.items) == len(node.items) - 1
            else [None] * (len(node.items) - 1)
        )
        branches, types = [], []
        for branch_node, branch_hint in zip(node.items[1:], hints, strict=True):
            with self.collecting() as effects:
                branch, t = self.check(branch_node, branch_hint, scope)
            self.read_only(effects, "par")
            branches.append(branch)
            types.append(t)
        return self.coerce(ir.Par(tuple(branches), node.span), TupleType(tuple(types)), expected, node.span)

    def read_only(self, effects: Effects, form: str) -> None:
        for capability, call_span in effects.calls:
            if capability.effect is Effect.WRITE:
                self.error(
                    "parallel-write", call_span,
                    f"{form} runs in parallel, so it cannot call {capability.id}, which writes.",
                    detail="Call writes outside par and par-map, one at a time.", help=form,
                )

    def form_par_map(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        if not self.arity(node, 3, 3, "{:limit 8} (fn [x] ...) xs"):
            return self.placeholder(span)
        options = self.options(node.items[1], "par-map", {"limit": "{:limit 8}"})
        limit = 1
        if options is not None:
            value = options.get("limit")
            if not isinstance(value, IntLiteral) or not 1 <= value.value <= self.budgets.concurrency:
                self.error(
                    "par-map-limit", node.items[1].span,
                    f"par-map takes {{:limit N}}, where N is an integer literal from 1 to {self.budgets.concurrency}.",
                    help="par-map",
                )
            else:
                limit = value.value
        items, items_type = self.check(node.items[3], None, scope)
        element = self.list_element(items_type, node.items[3], "par-map")
        with self.collecting() as effects:
            function, result = self.function_argument(node.items[2], [element], None, "par-map", scope)
        self.read_only(effects, "par-map")
        return self.coerce(ir.ParMap(limit, function, items, span), ListType(result), expected, span)

    def list_element(self, t: Type, node: Node, who: str) -> Type:
        resolved = self.resolve(t)
        if isinstance(resolved, ListType):
            return resolved.element
        if isinstance(resolved, Variable):
            element = self.fresh()
            self.unify(t, ListType(element))
            return element
        if not isinstance(resolved, Never):
            hint = (
                "text functions take strings; split one into a list with text.split or text.chars."
                if resolved == STRING
                else None
            )
            self.error(
                "type-mismatch", node.span,
                f"{who} takes a list here, but this is {describe(self.zonk(resolved))}.",
                detail=f"Expected: List<...>\nFound: {show_type(self.zonk(resolved))}"
                + (f"\n{hint}" if hint is not None else ""),
                help=who,
            )
        return NEVER


    def function_argument(
        self, node: Node, params: list[Type], expected_return: Type | None, who: str, scope: Mapping[str, Binding],
    ) -> tuple[ir.Lambda, Type]:
        span = node.span
        if is_function_form(node):
            assert isinstance(node, Form)
            if not self.check_fn_shape(node):
                return ir.Lambda((), ir.Literal(UNIT_VALUE, span), None, span), NEVER
            return self.instantiate(node, params, expected_return, who, scope, None)
        if isinstance(node, Symbol):
            binding = scope.get(node.name)
            if isinstance(binding, FunctionBinding):
                if node.name in self.instantiating:
                    self.error("unknown-function", span, f"{node.name} cannot call itself. Sx has no recursion.")
                    return ir.Lambda((), ir.Literal(UNIT_VALUE, span), None, span), NEVER
                lambda_ir, t = self.instantiate(binding.form, params, expected_return, who, binding.scope, node.name)
                return lambda_ir, t
            if binding is None and (node.name in PRELUDE or self.registry.function(node.name) is not None):
                return self.named_function(node, params, expected_return, who, scope)
        if isinstance(node, Keyword):
            self.habit(
                "keyword-call", span, "A keyword is not a function.", fix=f"(fn [r] (get r :{node.name}))", help="get",
            )
            return ir.Lambda((), ir.Literal(UNIT_VALUE, span), None, span), NEVER
        self.error(
            "type-mismatch", span, f"{who} takes a function here, such as (fn [x] ...), but this is {show(node)}.",
            help=who,
        )
        self.check(node, None, scope)
        return ir.Lambda((), ir.Literal(UNIT_VALUE, span), None, span), NEVER

    def named_function(
        self, node: Symbol, params: list[Type], expected_return: Type | None, who: str, scope: Mapping[str, Binding],
    ) -> tuple[ir.Lambda, Type]:
        names = [f"#{i}" for i in range(len(params))]
        inner = {**scope, **{name: ValueBinding(t) for name, t in zip(names, params, strict=True)}}
        call = Form((node, *(Symbol(name, node.span) for name in names)), node.span)
        with self.outside_tail():
            body, t = self.check(call, expected_return, inner)
        return ir.Lambda(tuple(ir.BindName(name) for name in names), body, None, node.span), t

    def instantiate(
        self, form: Form, params: list[Type], expected_return: Type | None, who: str, scope: Mapping[str, Binding],
        local: str | None,
    ) -> tuple[ir.Lambda, Type]:
        span = form.span
        self.instantiations += 1
        if self.instantiations > self.budgets.instantiations:
            if self.instantiations == self.budgets.instantiations + 1:
                self.error("too-large", span, "Local functions expand into more instantiations than the budget allows.")
            return ir.Lambda((), ir.Literal(UNIT_VALUE, span), local, span), NEVER
        param_nodes = form.items[1]
        assert isinstance(param_nodes, ListLiteral)
        if len(param_nodes.items) != len(params):
            given = len(param_nodes.items)
            self.error(
                "arity", param_nodes.span,
                f"This function takes {given} parameter{'s' if given != 1 else ''}, but {who} passes {len(params)}.",
                help=who if local is None else None,
            )
            return ir.Lambda((), ir.Literal(UNIT_VALUE, span), local, span), NEVER
        inner: dict[str, Binding] = dict(scope)
        binders = []
        for pattern, t in zip(param_nodes.items, params, strict=True):
            binder, inner = self.bind(pattern, t, inner, role=who, fix=self.record_fix(pattern, t, form.items[2]))
            binders.append(binder)
        if local is not None:
            self.instantiating.append(local)
        saved, self.tail = self.tail, None
        try:
            body, t = self.check(form.items[2], expected_return, inner)
        finally:
            self.tail = saved
            if local is not None:
                self.instantiating.pop()
        return ir.Lambda(tuple(binders), body, local, span), t

    def record_fix(self, pattern: Node, t: Type, body: Node) -> str | None:
        resolved = self.resolve(t)
        if not isinstance(pattern, ListLiteral) or not isinstance(resolved, RecordType) or not isinstance(body, Symbol):
            return None
        names = [item.name if isinstance(item, Symbol) else None for item in pattern.items]
        if body.name not in names or names.index(body.name) >= len(resolved.fields):
            return None
        return f"(fn [r] (get r :{resolved.fields[names.index(body.name)][0]}))"

    def apply_local(
        self, node: Form, binding: FunctionBinding, expected: Type | None, scope: Mapping[str, Binding],
    ) -> tuple[ir.Node, Type]:
        name = binding.name
        if name in self.instantiating:
            self.error("unknown-function", node.items[0].span, f"{name} cannot call itself. Sx has no recursion.")
            return self.placeholder(node.span)
        args = [self.check(arg, None, scope) for arg in node.items[1:]]
        lambda_ir, t = self.instantiate(binding.form, [t for _, t in args], expected, name, binding.scope, name)
        return self.coerce(ir.ApplyLocal(name, lambda_ir, tuple(a for a, _ in args), node.span), t, expected, node.span)

    def core(
        self, node: Form, function: Function, expected: Type | None, scope: Mapping[str, Binding],
    ) -> tuple[ir.Node, Type]:
        signature = " ".join(show_type(p) for p in function.params) + f" -> {show_type(function.output)}"
        if not self.arity(node, len(function.params), len(function.params), signature):
            return self.placeholder(node.span)
        args = []
        for arg, param in zip(node.items[1:], function.params, strict=True):
            value, t = self.check(arg, None, scope)
            value, _ = self.coerce(
                value, t, param, arg.span,
                message=f"{function.name} takes {describe(param)} here, but this is {describe(self.zonk(t))}.",
                help_name=function.name,
            )
            args.append(value)
        return self.coerce(ir.CoreCall(function, tuple(args), node.span), function.output, expected, node.span)


    def prelude(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        head = node.items[0]
        assert isinstance(head, Symbol)
        op = head.name
        span = node.span
        args = node.items[1:]
        if (
            op in HIGHER_ORDER
            and len(args) >= 2
            and not self.looks_like_function(args[0], scope)
            and self.looks_like_function(args[-1], scope)
        ):
            swapped = f"({op} {' '.join(show(a) for a in (args[-1], *args[:-1]))})"
            self.habit(
                "argument-order", node.span, f"{op} takes the function first, as in Clojure.", fix=swapped, help=op,
            )
            return self.placeholder(span)
        if op == "-":
            if not self.arity(node, 1, 2, "Int Int"):
                return self.placeholder(span)
            checked = [self.check(arg, INT, scope)[0] for arg in args]
            return self.coerce(ir.Prelude(op, tuple(checked), span), INT, expected, span)
        if op == "str":
            if not self.arity(node, 1, None, "String ..."):
                return self.placeholder(span)
            return self.prelude_str(node, expected, scope)
        if op in FIXED_ARITY and not self.arity(node, FIXED_ARITY[op], FIXED_ARITY[op], self.signature(op)):
            return self.placeholder(span)
        match op:
            case "+" | "*" | "quot" | "rem":
                checked = [self.check(arg, INT, scope)[0] for arg in args]
                return self.coerce(ir.Prelude(op, tuple(checked), span), INT, expected, span)
            case "=" | "not=":
                order = [1, 0] if isinstance(args[0], Symbol) and args[0].name[:1].isupper() else [0, 1]
                first, first_type = self.check(args[order[0]], None, scope)
                second, _ = self.check(args[order[1]], first_type, scope)
                pair = (first, second) if order == [0, 1] else (second, first)
                return self.coerce(ir.Prelude(op, pair, span), BOOL, expected, span)
            case "<" | "<=" | ">" | ">=":
                first, first_type = self.check(args[0], None, scope)
                second, _ = self.check(args[1], first_type, scope)
                resolved = self.resolve(first_type)
                if not isinstance(resolved, Never | Variable) and resolved not in {INT, STRING}:
                    hint = OPTIONAL_HINT if isinstance(resolved, OptionType) else None
                    self.error(
                        "type-mismatch", args[0].span,
                        f"{op} compares integers or strings, but this is {describe(self.zonk(resolved))}.",
                        detail=hint, help=op,
                    )
                return self.coerce(ir.Prelude(op, (first, second), span), BOOL, expected, span)
            case "not":
                return self.coerce(ir.Prelude(op, (self.check(args[0], BOOL, scope)[0],), span), BOOL, expected, span)
            case "and" | "or":
                checked = [self.check(arg, BOOL, scope)[0] for arg in args]
                return self.coerce(ir.Prelude(op, tuple(checked), span), BOOL, expected, span)
            case "count":
                items, t = self.check(args[0], None, scope)
                self.list_element(t, args[0], "count")
                return self.coerce(ir.Prelude(op, (items,), span), INT, expected, span)
            case "first":
                items, t = self.check(args[0], None, scope)
                element = self.list_element(t, args[0], "first")
                return self.coerce(ir.Prelude(op, (items,), span), element, expected, span)
            case "rest":
                items, t = self.check(args[0], None, scope)
                element = self.list_element(t, args[0], "rest")
                return self.coerce(ir.Prelude(op, (items,), span), ListType(element), expected, span)
            case "nth":
                items, t = self.check(args[0], None, scope)
                element = self.list_element(t, args[0], "nth")
                index, _ = self.check(args[1], INT, scope)
                return self.coerce(ir.Prelude(op, (items, index), span), element, expected, span)
            case "take":
                count, _ = self.check(args[0], INT, scope)
                items, t = self.check(args[1], None, scope)
                element = self.list_element(t, args[1], "take")
                return self.coerce(ir.Prelude(op, (count, items), span), ListType(element), expected, span)
            case "concat":
                first, t = self.check(args[0], None, scope)
                element = self.list_element(t, args[0], "concat")
                second, _ = self.check(args[1], ListType(element), scope)
                return self.coerce(ir.Prelude(op, (first, second), span), ListType(element), expected, span)
            case "flatten":
                items, t = self.check(args[0], None, scope)
                outer = self.list_element(t, args[0], "flatten")
                inner = (
                    self.list_element(outer, args[0], "flatten")
                    if not isinstance(self.resolve(outer), Never)
                    else NEVER
                )
                return self.coerce(ir.Prelude(op, (items,), span), ListType(inner), expected, span)
            case "contains?":
                items, t = self.check(args[0], None, scope)
                element = self.list_element(t, args[0], "contains?")
                value, _ = self.check(args[1], element, scope)
                return self.coerce(ir.Prelude(op, (items, value), span), BOOL, expected, span)
            case "reduce":
                initial, accumulator = self.check(args[1], None, scope)
                items, t = self.check(args[2], None, scope)
                element = self.list_element(t, args[2], "reduce")
                function, _ = self.function_argument(args[0], [accumulator, element], accumulator, "reduce", scope)
                return self.coerce(ir.Prelude(op, (function, initial, items), span), accumulator, expected, span)
        return self.higher_order(node, op, expected, scope)

    def signature(self, op: str) -> str:
        return {
            "=": "a b", "not=": "a b", "<": "a b", "<=": "a b", ">": "a b", ">=": "a b",
            "not": "Bool", "and": "Bool Bool", "or": "Bool Bool",
            "count": "xs", "first": "xs", "rest": "xs", "nth": "xs i", "take": "n xs", "concat": "xs ys",
            "flatten": "xss", "contains?": "xs x", "reduce": "f init xs",
        }.get(op, BINARY_SIGNATURE.get(op, "f xs"))

    def looks_like_function(self, node: Node, scope: Mapping[str, Binding]) -> bool:
        if is_function_form(node):
            return True
        if isinstance(node, Symbol):
            binding = scope.get(node.name)
            if isinstance(binding, FunctionBinding):
                return True
            return binding is None and (node.name in PRELUDE or self.registry.function(node.name) is not None)
        return False

    def higher_order(
        self, node: Form, op: str, expected: Type | None, scope: Mapping[str, Binding],
    ) -> tuple[ir.Node, Type]:
        span = node.span
        function_node, items_node = node.items[1], node.items[2]
        items, items_type = self.check(items_node, None, scope)
        element = self.list_element(items_type, items_node, op)
        match op:
            case "map":
                function, result = self.function_argument(function_node, [element], None, op, scope)
                return self.coerce(ir.Prelude(op, (function, items), span), ListType(result), expected, span)
            case "filter" | "any?" | "all?":
                function, _ = self.function_argument(function_node, [element], BOOL, op, scope)
                t: Type = ListType(element) if op == "filter" else BOOL
                return self.coerce(ir.Prelude(op, (function, items), span), t, expected, span)
            case "keep":
                inner = self.fresh()
                function, _ = self.function_argument(function_node, [element], OptionType(inner), op, scope)
                return self.coerce(ir.Prelude(op, (function, items), span), ListType(inner), expected, span)
            case "sort-by":
                self.sort_key_owner = node
                try:
                    function, key = self.function_argument(function_node, [element], None, op, scope)
                finally:
                    self.sort_key_owner = None
                if not self.is_sort_key(key):
                    hint = None
                    if (
                        head_name(items_node) == "filter"
                        and isinstance(items_node, Form)
                        and len(items_node.items) == 3
                        and "none" in show(items_node.items[1])
                    ):
                        hint = (
                            "To drop none values and unwrap the rest, "
                            f"use (keep (fn [x] x) {show(items_node.items[2])})."
                        )
                    self.error(
                        "type-mismatch", function_node.span,
                        "sort-by needs a key that is an Int, String, Bool, or a tuple of them.",
                        detail=f"Expected: a sort key\nFound: {show_type(self.zonk(key))}"
                        + (f"\n{hint}" if hint is not None else ""),
                        help="keep" if hint is not None else "sort-by",
                    )
                return self.coerce(ir.Prelude(op, (function, items), span), ListType(element), expected, span)
            case "distinct-by":
                function, _ = self.function_argument(function_node, [element], None, op, scope)
                return self.coerce(ir.Prelude(op, (function, items), span), ListType(element), expected, span)
            case "group-by":
                function, key = self.function_argument(function_node, [element], None, op, scope)
                t = ListType(TupleType((key, ListType(element))))
                return self.coerce(ir.Prelude(op, (function, items), span), t, expected, span)
        raise AssertionError(op)

    def prelude_str(self, node: Form, expected: Type | None, scope: Mapping[str, Binding]) -> tuple[ir.Node, Type]:
        span = node.span
        owner = self.sort_key_owner
        hint = None
        if (
            owner is not None
            and isinstance(owner.items[1], Form)
            and len(owner.items[1].items) == 3
            and owner.items[1].items[2] is node
        ):
            params = owner.items[1].items[1]
            parts = [show(arg) for arg in node.items[1:] if not isinstance(arg, StringLiteral)]
            hint = f"To sort by several keys, return a tuple: (fn {show(params)} [{' '.join(parts)}])"
        checked = []
        for arg in node.items[1:]:
            value, t = self.check(arg, None, scope)
            value, _ = self.coerce(
                value, t, STRING, arg.span,
                message=f"str joins strings, but this argument is {describe(self.zonk(t))}.",
                detail_hint=(
                    hint
                    if hint is not None
                    else ("Convert an Int with text.from-int." if self.resolve(t) == INT else None)
                ),
                help_name="sort-by" if hint is not None else "str",
            )
            checked.append(value)
        return self.coerce(ir.Prelude("str", tuple(checked), span), STRING, expected, span)


def program_span(source: str) -> Span:
    lines = source.split("\n")
    return Span(Position(1, 1), Position(len(lines), len(lines[-1]) + 1))
