from __future__ import annotations

from collections.abc import Iterable, Mapping
from dataclasses import dataclass
from importlib.resources import files
from typing import Any

from sx.registry import Capability, Effect, Function, Registry, load_declarations
from sx.types import CALL_ERROR_KINDS, EnumType, RecordType, Type, show_definition, show_type


@dataclass(frozen=True, slots=True)
class Entry:
    name: str
    signature: str
    description: str
    example: str
    result: str


def entry(name: str, signature: str, description: str, example: str, result: str) -> Entry:
    return Entry(name, signature, description, example, result)


FORMS = (
    entry("let", "(let [name value ...] body)", "Binds names, each visible to later bindings and the body. "
          "A fn value binds a local function, and _ runs a value for its effect.",
          "(let [a 1 b (+ a 1) double (fn [x] (* 2 x))] (double b))", "4"),
    entry("if", "(if condition then else)", "Chooses a branch. The condition is true or false, or an "
          "optional, which tests that a value is present. When the condition is an optional name, the "
          "then-branch sees the value inside it.",
          '(let [n (some 3)] (if n (+ n 1) 0))', "4"),
    entry("match", "(match value pattern result ...)", "Chooses the first clause whose pattern matches. "
          "Patterns: (some x) none (ok x) (err e) Variant (Variant x) _, or a name that binds the "
          "whole value like _.",
          "(match (some 3) (some n) (+ n 1) none 0)", "4"),
    entry("fn", "(fn [param ...] body)", "A function, as an argument to map, filter, and the like, "
          "or as the value of a let binding.", "(map (fn [x] (+ x 1)) [1 2])", "[2 3]"),
    entry("get", "(get record :field) or (get record :field default)", "Reads a record field. With a default, "
          "the field must be optional, and the default replaces none.",
          '(get {:nick none} :nick "anon")', '"anon"'),
    entry("call", "(call tool.name {:field value})", "Calls a tool with an input record.",
          '(count (call people.list {:team "infra"}))', "3"),
    entry("attempt", "(attempt expression)", "(ok value), or (err {:kind K :message m}) if a tool call failed.",
          '(attempt (call metrics.uptime {:host "gone"}))', '(err {:kind NotFound :message "no such host"})'),
    entry("retry", "(retry {:delays [1s 2s]} expression)", "Evaluates the expression again after each delay "
          "while it fails with a transient kind: Timeout, Unavailable, or RateLimited. :on [Kind ...] chooses "
          "the kinds, and {:backoff [100ms 5s] :within 30s} computes doubling delays.",
          '(get (retry {:delays [1s 2s]} (call metrics.uptime {:host "flaky"})) :seconds)', "60"),
    entry("wait-until", "(wait-until {:within 2m :every 5s} (fn [v] done?) expression)",
          "Evaluates the expression until the function returns true for its value, and returns that value. "
          "It fails with kind Timeout after :within.",
          '(wait-until {:within 1m :every 5s} (fn [s] (= s "done")) (call job.state {:id "j1"}))', '"done"'),
    entry("loop", "(loop {:max 20} [name value ...] body)", "Binds like let and evaluates the body. "
          "(recur value ...) as its last step runs the body again with new values, at most :max times.",
          "(loop {:max 10} [n 0 total 0] (if (= n 4) total (recur (+ n 1) (+ total n))))", "6"),
    entry("recur", "(recur value ...)", "Runs the enclosing loop body again with new values for its names. "
          "It must be the last step of the body.",
          "(loop {:max 5} [n 3] (if (= n 0) \"liftoff\" (recur (- n 1))))", '"liftoff"'),
    entry("par", "(par expression ...)", "Evaluates tool reads in parallel and returns a tuple in source order.",
          '(let [[a b] (par (call metrics.uptime {:host "db1"}) (call metrics.uptime {:host "db2"}))] '
          "(+ (get a :seconds) (get b :seconds)))", "300"),
    entry("par-map", "(par-map {:limit 8} (fn [x] ...) xs)", "Maps over a list in parallel, at most :limit at a "
          "time, with reads only. Results keep list order.",
          '(par-map {:limit 2} (fn [h] (get (call metrics.uptime {:host h}) :seconds)) ["db1" "db2"])',
          "[100 200]"),
    entry("some", "(some value)", "An optional value that is present.", "(some 1)", "(some 1)"),
    entry("none", "none", "An optional value that is absent. Sx has no nil.", "(match none (some x) x none 0)", "0"),
    entry("ok", "(ok value)", "A successful result.", "(match (ok 1) (ok x) x (err e) 0)", "1"),
    entry("err", "(err value)", "A failed result.", '(match (err "bad") (ok x) "fine" (err e) e)', '"bad"'),
)

PRELUDE = (
    entry("+", "(+ a b) -> Int", "Adds two integers. Overflow stops the program.", "(+ 2 3)", "5"),
    entry("-", "(- a b) -> Int, (- x) -> Int", "Subtracts, or negates one integer.", "(- 10 4)", "6"),
    entry("*", "(* a b) -> Int", "Multiplies two integers.", "(* 6 7)", "42"),
    entry("quot", "(quot a b) -> Int", "Divides, truncating toward zero. Division by zero stops the program.",
          "(quot -7 2)", "-3"),
    entry("rem", "(rem a b) -> Int", "The remainder of quot.", "(rem -7 2)", "-1"),
    entry("=", "(= a b) -> Bool", "True when two values of one type are equal.", "(= [1 2] [1 2])", "true"),
    entry("not=", "(not= a b) -> Bool", "True when two values differ.", '(not= "a" "b")', "true"),
    entry("<", "(< a b) -> Bool", "Compares integers, or strings by code point.", "(< 1 2)", "true"),
    entry("<=", "(<= a b) -> Bool", "Compares integers or strings.", "(<= 2 2)", "true"),
    entry(">", "(> a b) -> Bool", "Compares integers or strings.", '(> "b" "a")', "true"),
    entry(">=", "(>= a b) -> Bool", "Compares integers or strings.", "(>= 1 2)", "false"),
    entry("not", "(not b) -> Bool", "Negates a boolean.", "(not false)", "true"),
    entry("and", "(and a b) -> Bool", "True when both are true. Both arguments always evaluate.",
          "(and true false)", "false"),
    entry("or", "(or a b) -> Bool", "True when either is true. Both arguments always evaluate.",
          "(or true false)", "true"),
    entry("str", "(str s ...) -> String", "Joins strings.", '(str "a" "b" "c")', '"abc"'),
    entry("count", "(count xs) -> Int", "The number of elements.", "(count [1 2 3])", "3"),
    entry("first", "(first xs) -> T", "The first element. An empty list stops the program.", "(first [1 2 3])", "1"),
    entry("rest", "(rest xs) -> List<T>", "Every element after the first.", "(rest [1 2 3])", "[2 3]"),
    entry("nth", "(nth xs i) -> T", "The element at index i, from 0. A missing index stops the program.",
          "(nth [10 20 30] 1)", "20"),
    entry("take", "(take n xs) -> List<T>", "The first n elements.", "(take 2 [1 2 3])", "[1 2]"),
    entry("concat", "(concat xs ys) -> List<T>", "Joins two lists.", "(concat [1] [2 3])", "[1 2 3]"),
    entry("flatten", "(flatten xss) -> List<T>", "Joins a list of lists.", "(flatten [[1] [2 3]])", "[1 2 3]"),
    entry("contains?", "(contains? xs x) -> Bool", "True when the list holds the value.", "(contains? [1 2] 2)",
          "true"),
    entry("map", "(map f xs) -> List<U>", "Applies f to each element, in order.", "(map (fn [x] (* x x)) [1 2 3])",
          "[1 4 9]"),
    entry("filter", "(filter f xs) -> List<T>", "Keeps the elements for which f returns true.",
          "(filter (fn [x] (> x 1)) [1 2 3])", "[2 3]"),
    entry("keep", "(keep f xs) -> List<U>", "Applies f, which returns an optional, and keeps the some values "
          "unwrapped.", "(keep (fn [x] (if (> x 1) x none)) [1 2 3])", "[2 3]"),
    entry("any?", "(any? f xs) -> Bool", "True when f returns true for some element. Stops at the first.",
          "(any? (fn [x] (> x 2)) [1 2 3])", "true"),
    entry("all?", "(all? f xs) -> Bool", "True when f returns true for every element. Stops at the first false.",
          "(all? (fn [x] (> x 2)) [1 2 3])", "false"),
    entry("reduce", "(reduce f init xs) -> A", "Combines the elements into one value, left to right.",
          "(reduce + 0 [1 2 3])", "6"),
    entry("sort-by", "(sort-by f xs) -> List<T>", "Sorts ascending by a key: an integer, a string, a boolean, "
          "or a tuple of keys compared in order. Negate an integer key to sort descending.",
          '(sort-by (fn [p] [(- (get p :n)) (get p :name)]) [{:name "b" :n 1} {:name "a" :n 2}])',
          '[{:name "a" :n 2} {:name "b" :n 1}]'),
    entry("distinct-by", "(distinct-by f xs) -> List<T>", "Keeps the first element for each key.",
          "(distinct-by (fn [x] (rem x 2)) [1 2 3 4])", "[1 2]"),
    entry("group-by", "(group-by f xs) -> List<Tuple<K, List<T>>>", "Groups elements by key, as [key items] "
          "tuples in first-seen order.", "(group-by (fn [x] (rem x 2)) [1 2 3])", "[[1 [1 3]] [0 [2]]]"),
)

TYPES = (
    entry("Option", "Option<T>: (some x) or none", "An optional value. Wherever an optional is expected, a plain "
          "value also works and becomes (some value).", "(map (fn [x] (if (> x 1) x none)) [1 2])", "[none (some 2)]"),
    entry("Result", "Result<T, CallError>: (ok x) or (err e)", "What attempt returns.",
          '(match (attempt (call metrics.uptime {:host "db1"})) (ok m) (get m :seconds) (err e) 0)', "100"),
    entry("CallError", "Record {kind: CallErrorKind, message: String}", "Why a tool call failed. Kinds: "
          + ", ".join(CALL_ERROR_KINDS) + ". Branch on the kind, never the message.",
          '(match (attempt (call metrics.uptime {:host "gone"})) (ok m) "up" (err e) (match (get e :kind) '
          'NotFound "missing" _ "other"))', '"missing"'),
    entry("Tuple", "Tuple<A, B, ...>: [a b ...]", "A fixed-size group of values of different types, from par, "
          "group-by, and bracket literals of mixed types or that a tuple pattern takes apart.",
          '(let [[n s] [1 "a"]] s)', '"a"'),
)

ENTRIES: Mapping[str, Entry] = {e.name: e for e in (*FORMS, *PRELUDE, *TYPES)}

EXAMPLE_DECLARATIONS: Mapping[str, Any] = {
    "types": {"User": "Record {name: String, active: Bool}", "File": "Record {bytes: Int}"},
    "capabilities": [
        {"id": "people.list", "effect": "read", "input": "Record {team: String}", "output": "List<User>",
         "description": "Lists the people on a team."},
        {"id": "metrics.uptime", "effect": "read", "input": "Record {host: String}",
         "output": "Record {seconds: Int}", "description": "Reads a host's uptime."},
        {"id": "storage.files", "effect": "read", "input": "Record {page: Int}",
         "output": "Record {files: List<File>, last: Bool}", "description": "Reads one page of files."},
        {"id": "job.state", "effect": "read", "input": "Record {id: String}", "output": "String",
         "description": "Reads a job's state."},
    ],
}


def uptime(host: str, *results: Mapping[str, Any]) -> dict[str, Any]:
    return {"capability": "metrics.uptime", "input": {"host": host}, "results": list(results)}


EXAMPLE_RESPONSES: tuple[Mapping[str, Any], ...] = (
    {"capability": "people.list", "input": {"team": "infra"}, "results": [{"output": [
        {"name": "lin", "active": True}, {"name": "ada", "active": True}, {"name": "bo", "active": False},
    ]}]},
    uptime("db1", {"output": {"seconds": 100}}),
    uptime("db2", {"output": {"seconds": 200}}),
    uptime("web1", {"error": {"kind": "Timeout", "message": "no answer"}}),
    uptime("gone", {"error": {"kind": "NotFound", "message": "no such host"}}),
    uptime("flaky", {"error": {"kind": "Unavailable", "message": "502"}}, {"output": {"seconds": 60}}),
    {"capability": "storage.files", "input": {"page": 1}, "results": [{"output": {
        "files": [{"bytes": 10}, {"bytes": 20}], "last": False}}]},
    {"capability": "storage.files", "input": {"page": 2}, "results": [{"output": {
        "files": [{"bytes": 5}], "last": True}}]},
    {"capability": "job.state", "input": {"id": "j1"}, "results": [{"output": "running"}, {"output": "done"}]},
)


def example_registry() -> Registry:
    registry = Registry()
    load_declarations(registry, EXAMPLE_DECLARATIONS)
    return registry


GUIDE_LEVELS = ("minimal", "standard", "full")

MINIMAL_HEADER = """# Sx

Sx calls tools and combines their results. A program is one expression; its value is the result.
It looks like Clojure, but only the forms and functions below exist.

Values: true false, integers (no fractions), "strings", [lists], {:field value} records (keywords
only name fields), [1 "a"] tuples (mixed types), (some x) and none (no nil; a plain value works
where an optional is expected), (ok x) and (err e). Durations such as 250ms 5s 2m 1h appear only in
retry and wait-until options. Names may be kebab-case, camelCase, or snake_case. Comments start
with ;.
"""


def first_sentence(text: str) -> str:
    return text.split(". ")[0].rstrip(".") + "."


def guide(level: str = "standard") -> str:
    standard = files("sx").joinpath("guide.md").read_text()
    if level == "standard":
        return standard
    if level == "minimal":
        lines = [MINIMAL_HEADER, "Forms:"]
        shown = [e for e in FORMS if e.name not in {"some", "ok", "err"}]
        lines += [f"  {e.signature}  {first_sentence(e.description)}" for e in shown]
        lines += ["", "Functions (function arguments come first):"]
        lines += [f"  {e.signature}" for e in PRELUDE]
        kinds = " ".join(CALL_ERROR_KINDS)
        lines += ["", f"Error kinds for attempt, retry :on, and (get e :kind): {kinds}.", ""]
        return "\n".join(lines)
    if level == "full":
        lines = [standard.rstrip(), "", "## More examples", ""]
        for e in (*FORMS, *PRELUDE, *TYPES):
            lines.append(f"- `{e.example}` returns `{e.result}`")
        return "\n".join(lines) + "\n"
    raise ValueError(f"unknown guide level {level!r}; choose one of {', '.join(GUIDE_LEVELS)}")


def render_entry(e: Entry) -> str:
    return f"{e.signature}\n{e.description}\nExample: {e.example}\nResult: {e.result}\n"


def render_function(function: Function) -> str:
    params = " ".join(show_type(p) for p in function.params)
    lines = [f"({function.name} {params}) -> {show_type(function.output)}", function.description]
    if function.example is not None:
        lines += [f"Example: {function.example.program}", f"Result: {function.example.result}"]
    return "\n".join(lines) + "\n"


def render_capability(capability: Capability, registry: Registry) -> str:
    effect = capability.effect.value
    if capability.effect is Effect.WRITE:
        effect += ", idempotent" if capability.idempotent else ", not idempotent"
    lines = [
        f"(call {capability.id} {show_type(capability.input)}) -> {show_type(capability.output)}   [{effect}]",
        capability.description,
    ]
    lines += [f"  {line}" for line in named_types([capability.input, capability.output], registry)]
    return "\n".join(lines) + "\n"


def named_types(types: Iterable[Type], registry: Registry) -> list[str]:
    wanted = {show_type(t) for t in types}
    lines = []
    for name, t in registry.types.items():
        if not any(name in text for text in wanted):
            continue
        if isinstance(t, EnumType):
            variants = " | ".join(v if p is None else f"({v} {show_type(p)})" for v, p in t.variants)
            lines.append(f"{name} = {variants}")
        elif isinstance(t, RecordType):
            lines.append(f"{name} = {show_definition(t)}")
    return lines


def lookup(name: str, registry: Registry) -> str | None:
    if name in ENTRIES:
        return render_entry(ENTRIES[name])
    function = registry.function(name)
    if function is not None:
        return render_function(function)
    capability = registry.capability(name)
    if capability is not None:
        return render_capability(capability, registry)
    t = registry.types.get(name)
    if t is not None:
        return "\n".join(named_types([t], registry) or [f"{name} = {show_type(t)}"]) + "\n"
    return None


def names(registry: Registry) -> list[str]:
    return [*ENTRIES, *registry.functions, *registry.capabilities, *registry.types]


def tool_listing(registry: Registry, capability_ids: Iterable[str], core_ids: Iterable[str]) -> str:
    lines: list[str] = []
    types_seen = [t for name, t in registry.types.items() if name.split(".")[0] not in set(core_ids)]
    records = [(n, t) for n, t in registry.types.items() if isinstance(t, RecordType) and t in types_seen]
    enums = [(n, t) for n, t in registry.types.items() if isinstance(t, EnumType)]
    if records:
        lines += ["Types:", *(f"  {name} = {show_definition(t)}" for name, t in records), ""]
    if enums:
        lines.append("Enums (match on their variants):")
        for name, t in enums:
            assert isinstance(t, EnumType)
            variants = " | ".join(v if p is None else f"({v} {show_type(p)})" for v, p in t.variants)
            lines.append(f"  {name} = {variants}")
        lines.append("")
    lines.append("Tools:")
    for capability_id in capability_ids:
        capability = registry.capability(capability_id)
        assert capability is not None
        lines.append("  " + render_capability(capability, registry).split("\n", 2)[0])
        lines.append(f"      {capability.description}")
    lines.append("")
    functions = [f for name, f in registry.functions.items() if name.split(".")[0] in set(core_ids)]
    if functions:
        lines.append("Functions:")
        for name, t in registry.types.items():
            if name.split(".")[0] in set(core_ids):
                lines.append(f"  {name} = {show_definition(t)}")
        for function in functions:
            params = " ".join(show_type(p) for p in function.params)
            lines.append(f"  ({function.name} {params}) -> {show_type(function.output)}   {function.description}")
        lines.append("")
    return "\n".join(lines)
