from __future__ import annotations

import random
from dataclasses import dataclass


@dataclass(frozen=True, slots=True)
class Mutant:
    name: str
    code: str
    snippet: str


MUTANTS: tuple[Mutant, ...] = (
    Mutant("while", "ts-while", "let m = 0; while (m < 3) { m += 1; }"),
    Mutant("do-while", "ts-while", "let m = 0; do { m += 1; } while (m < 3);"),
    Mutant("uncounted-for", "ts-while", "let m = 0; for (;;) { m += 1; if (m > 2) { break; } }"),
    Mutant("date", "ts-date", "const m = new Date();"),
    Mutant("random", "ts-nondeterministic", "const m = Math.random();"),
    Mutant("set-timeout", "ts-nondeterministic", "setTimeout(() => 1, 10);"),
    Mutant("undefined", "ts-undefined", "const m = undefined;"),
    Mutant("loose-equality", "ts-loose-equality", "const m = 1 == 1;"),
    Mutant("strict-null", "js-null-equality", "const o = [1].length > 0 ? \"a\" : null; const m = o === null;"),
    Mutant("switch", "ts-switch", "switch (1) { case 1: break; }"),
    Mutant("regexp", "ts-regexp", "const m = /a/.test(\"a\");"),
    Mutant("class", "ts-class", "class M { }"),
    Mutant("finally", "ts-finally", "try { } finally { }"),
    Mutant("recursion", "ts-recursion", "function m(n: number): number { return n > 0 ? m(n - 1) : 0; }"),
    Mutant("import", "ts-import-export", "import x from \"y\";"),
    Mutant("power", "ts-operator", "const m = 2 ** 3;"),
    Mutant("bitwise", "ts-operator", "const m = 5 & 3;"),
    Mutant("tools-alias", "ts-tools-escape", "const m = tools;"),
    Mutant("tools-index", "ts-tools-escape", "const m = tools[\"items\"];"),
    Mutant("pop", "ts-mutating-method", "const m = [1, 2]; m.pop();"),
    Mutant("splice", "ts-mutating-method", "const m = [1, 2]; m.splice(0, 1);"),
    Mutant("for-in", "ts-for-in", "for (const k in { a: 1 }) { }"),
    Mutant("eval", "ts-eval", "eval(\"1\");"),
    Mutant("label", "ts-label", "m: for (const x of [1]) { break m; }"),
    Mutant("nan", "ts-unknown-builtin", "const m = NaN;"),
    Mutant("number-call", "ts-unknown-builtin", "const m = Number(\"1\");"),
    Mutant("json-untyped", "ts-json-parse-type", "const m = JSON.parse(\"1\");"),
    Mutant("or-assign", "ts-operator", "let m = 0; m ||= 1;"),
    Mutant("float-remainder", "js-float-remainder", "const m = 1.5 % 2;"),
    Mutant("unsafe-literal", "ts-unsafe-integer", "const m = 9007199254740993;"),
    Mutant("throw-string", "ts-throw", "if (1 > 2) { throw \"x\"; }"),
    Mutant("unawaited-call", "ts-promise-value", "const m = tools.items.count({});"),
    Mutant("locale", "ts-locale", "const m = \"a\".localeCompare(\"b\");"),
    Mutant("sort-numbers", "ts-sort-comparator", "const m = [3, 1, 2].slice(); m.sort();"),
    Mutant("generator", "ts-generator", "function* g() { yield 1; }"),
    Mutant("enum", "ts-enum", "enum E { A }"),
    Mutant("function-value", "ts-function-value", "const f = () => 1; const m = [f];"),
    Mutant("spread-call", "ts-spread", "const m = Math.max(...[1, 2]);"),
    Mutant("duplicate-key", "ts-key", "const m = { a: 1, a: 2 };"),
    Mutant("default-parameter", "ts-param", "function m(a = 1) { return a; }"),
    Mutant("console-error", "ts-console", "console.error(\"x\");"),
    Mutant("optional-chain-value", "ts-optional-chain", "const o = { a: null }; const m = o?.a; console.log(m);"),
    Mutant("while-in-loop-body", "ts-while", "for (const x of [1]) { let k = 0; while (k < x) { k += 1; } }"),
    Mutant("write-to-parameter", "ts-mutation-pattern", "function m(a: number[]) { a[0] = 1; return a; }"),
    Mutant("increment-in-expression", "ts-mutation-pattern", "let m = 0; const n = [1, 2][m++];"),
    Mutant("closure-assignment", "ts-mutation-escape", "let m = 0; const f = () => m + 1; m = 2;"),
    Mutant(
        "optional-read-without-exit", "type-mismatch",
        "const mo = [1].length > 0 ? \"a\" : null; if (mo != null) { } const m = mo.length;",
    ),
    Mutant("optional-read-in-falsy-branch", "type-mismatch", "const mo = [1].length > 0 ? \"a\" : null; const m = mo ? 1 : mo.length;"),
    Mutant("optional-read-in-null-branch", "type-mismatch", "const mo = [1].length > 0 ? \"a\" : null; const m = mo == null ? mo.length : 0;"),
    Mutant("optional-read-after-reassignment", "type-mismatch", "let mo = [1].length > 0 ? \"a\" : null; if (mo != null) { mo = null; const m = mo.length; }"),
    Mutant(
        "optional-read-under-another-test", "type-mismatch",
        "const mo = [1].length > 0 ? \"a\" : null; const mp = [1].length > 0 ? \"b\" : null; const m = mp != null ? mo.length : 0;",
    ),
    Mutant("optional-narrowed-by-or", "type-mismatch", "const mo = [1].length > 0 ? \"a\" : null; const m = mo != null || mo.length > 0;"),
    Mutant("variant-payload-without-test", "not-a-record", "const mr = await tools.items.review({ id: \"i0\" }); const m = mr.value;"),
    Mutant(
        "variant-payload-after-empty-test", "not-a-record",
        "const mr = await tools.items.review({ id: \"i0\" }); if (mr.tag === \"Score\") { } const m = mr.value;",
    ),
    Mutant("variant-tag-of-a-mixed-enum", "not-a-record", "const mr = await tools.items.review({ id: \"i0\" }); const m = mr.tag;"),
    Mutant(
        "variant-payload-of-another-variant", "not-a-record",
        "const mr = await tools.items.review({ id: \"i0\" }); const m = mr.tag === \"Score\" ? 0 : mr.value.level;",
    ),
    Mutant("variant-payload-as-string", "js-equality", "const mr = await tools.items.review({ id: \"i0\" }); const m = mr === \"Score\";"),
    Mutant(
        "field-read-without-exit", "type-mismatch",
        "const mi = await tools.items.get({ id: \"i0\" }); if (mi.note != null) { } const m = mi.note.length;",
    ),
    Mutant(
        "field-read-after-write", "type-mismatch",
        "let mi = { note: [1].length > 0 ? \"a\" : null }; if (mi.note != null) { mi.note = null; const m = mi.note.length; }",
    ),
    Mutant(
        "field-read-of-another-item", "type-mismatch",
        "const mi = await tools.items.get({ id: \"i0\" }); const mj = await tools.items.get({ id: \"i1\" }); const m = mi.note != null ? mj.note.length : 0;",
    ),
    Mutant(
        "map-read-used-as-present", "type-mismatch",
        "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } const mq = \"a\"; const m = mm[mq] + 1;",
    ),
    Mutant(
        "map-read-field-without-test", "not-a-record",
        "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = { n: 1 }; } const mq = \"a\"; const m = mm[mq].n;",
    ),
    Mutant(
        "map-compound-update", "type-mismatch",
        "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } for (const mk of [\"a\"]) { mm[mk] += 1; }",
    ),
    Mutant("map-number-key", "type-mismatch", "const mm = {}; for (const mk of [1, 2]) { mm[mk] = 1; }"),
    Mutant("map-two-value-types", "type-mismatch", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; mm[mk] = \"x\"; }"),
    Mutant("map-element-push", "ts-mutation-pattern", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = [1]; } for (const mk of [\"a\"]) { mm[mk].push(2); }"),
    Mutant("map-keys-unsorted", "ts-map-order", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } const m = Object.keys(mm);"),
    Mutant("map-stringified", "ts-map-order", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } console.log(JSON.stringify(mm));"),
    Mutant("map-spread", "ts-spread", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } const m = { ...mm };"),
    Mutant("map-delete", "ts-operator", "const mm = {}; for (const mk of [\"a\"]) { mm[mk] = 1; } delete mm[\"a\"];"),
    Mutant("record-computed-key", "ts-dynamic-key", "const mr = { a: 1 }; const mk = \"a\"; const m = mr[mk];"),
    Mutant("string-index", "ts-string-index", "const ms = \"abc\"; const m = ms[1];"),
    Mutant("array-element-assignment", "ts-mutation-pattern", "const mx = [1, 2]; mx[0] = 5;"),
    Mutant("array-element-compound", "ts-mutation-pattern", "const mx = [1, 2]; for (let mi = 0; mi < 2; mi++) { mx[mi] += 1; }"),
    Mutant("concat-two-arguments", "ts-unknown-builtin", "const mx = [1]; const m = mx.concat([2], [3]);"),
    Mutant("differently-keyed-branches", "branch-mismatch", "const mq = [1].length > 0 ? {} : { a: 1 };"),
    Mutant("tool-name", "unknown-tool", "await tools.items.nope({});"),
)


def mutate(program: str, rng: random.Random) -> tuple[str, Mutant]:
    mutant = rng.choice(MUTANTS)
    return mutant.snippet + "\n" + program, mutant
