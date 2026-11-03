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
    Mutant("tool-name", "unknown-tool", "await tools.items.nope({});"),
)


def mutate(program: str, rng: random.Random) -> tuple[str, Mutant]:
    mutant = rng.choice(MUTANTS)
    return mutant.snippet + "\n" + program, mutant
