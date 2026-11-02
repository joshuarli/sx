mod common;

use common::codes;

fn rejects(code: &str, source: &str) {
    let found = codes(source);
    assert!(found.contains(&code), "expected {code} for:\n{source}\nfound {found:?}");
}

fn accepts(source: &str) {
    let found = codes(source);
    assert!(found.is_empty(), "expected acceptance for:\n{source}\nfound {found:?}");
}

#[test]
fn syntax_and_modules() {
    rejects("ts-syntax", "const x = ;");
    rejects("ts-syntax", "const s = \"abc;");
    rejects("ts-import-export", "import fs from \"fs\";\nreturn 1;");
    rejects("ts-import-export", "export const x = 1;");
    rejects("ts-import-export", "const fs = require(\"fs\");");
    rejects("ts-eval", "const m = await import(\"x\");");
    rejects("ts-eval", "eval(\"1\");");
}

#[test]
fn declarations_and_classes() {
    rejects("ts-class", "class A {}");
    rejects("ts-class", "const o = new Foo();");
    rejects("ts-class", "const a = this;");
    rejects("ts-generator", "function* g() { yield 1; }");
    rejects("ts-generator", "for await (const x of xs) {}");
    rejects("ts-enum", "enum Color { Red }");
    rejects("ts-enum", "namespace N { }");
    rejects("ts-unsupported", "var x = 1;");
    rejects("ts-unsupported", "const f = function () { return 1; };");
}

#[test]
fn loops_and_statements() {
    rejects("ts-while", "while (true) { }");
    rejects("ts-while", "do { } while (true);");
    rejects("ts-while", "for (;;) { }");
    rejects("ts-for-in", "for (const k in {}) { }");
    rejects("ts-switch", "switch (1) { case 1: break; }");
    rejects("ts-label", "outer: for (const x of []) { }");
    rejects("ts-label", "debugger;");
    rejects("ts-finally", "try { 1; } catch { 2; } finally { 3; }");
    rejects("ts-counted-loop", "for (let i = 0.5; i < 3; i++) { }");
    rejects("ts-counted-loop", "for (let i = 0; i < 3; i--) { }");
    rejects("ts-counted-loop", "let n = 3;\nfor (let i = 0; i < n; i++) { n = 2; }");
    rejects("ts-counted-loop", "for (let i = 0; i < 3; i++) { i = 5; }");
    rejects("ts-counted-loop", "for (let i = 0; i < (await tools.fs.list({ path: \".\" })).length; i++) { }");
    rejects("ts-counted-loop", "for (let i = 0; i < 3; i += 0.5) { }");
    accepts("const xs = [1, 2];\nlet s = 0;\nfor (let i = 0; i < xs.length; i++) { s += xs[i]; }\nreturn s;");
}

#[test]
fn operators_and_literals() {
    rejects("ts-operator", "const x = 2 ** 3;");
    rejects("ts-operator", "const x = 1 & 2;");
    rejects("ts-operator", "const x = ~1;");
    rejects("ts-operator", "const o = {}; const x = \"a\" in o;");
    rejects("ts-operator", "let a = 1; a ||= 2;");
    rejects("ts-operator", "let a = 1; a ??= 2;");
    rejects("ts-operator", "const x = (1, 2);");
    rejects("ts-loose-equality", "const x = 1 == 2;");
    rejects("ts-instanceof", "const x = [] instanceof Array;");
    rejects("ts-undefined", "const x = undefined;");
    rejects("ts-undefined", "const x = void 0;");
    rejects("ts-undefined", "const a = \"x\"; const b = a === undefined;");
    rejects("ts-undefined", "const a = 1; const b = typeof a === \"undefined\";");
    rejects("ts-unsafe-integer", "const x = 9007199254740993;");
    rejects("ts-regexp", "const r = /a+/;");
    rejects("ts-regexp", "const s = \"a\".match(\"a\");");
    rejects("ts-unsupported-class", "const x = 10n;");
    rejects("ts-unsupported-class", "const s = new Set();");
    rejects("ts-unknown-builtin", "const x = NaN;");
    rejects("ts-unknown-builtin", "const x = Number(\"1\");");
    rejects("ts-unknown-builtin", "const x = parseFloat(\"1\");");
    rejects("ts-unknown-builtin", "const x = parseInt(\"1\");");
    rejects("ts-unknown-builtin", "const x = (1.5).toFixed(2);");
}

#[test]
fn keys_params_and_casts() {
    rejects("ts-key", "const o = { 1: \"a\" };");
    rejects("ts-key", "const k = \"a\"; const o = { [k]: 1 };");
    rejects("ts-key", "const o = { a: 1, a: 2 };");
    rejects("ts-key", "const o = { \"7\": 1 };");
    rejects("ts-spread", "const f = (a, b) => a; const xs = [1, 2]; f(...xs);");
    rejects("ts-spread", "const r = { a: 1 }; const o = { ...(await tools.fs.list({ path: \".\" })) };");
    rejects("ts-param", "function f(a = 1) { return a; }");
    rejects("ts-param", "function f(...a) { return a; }");
    rejects("ts-param", "const { a = 1 } = { a: 2 };");
    rejects("ts-cast", "const x = 1 as number;");
    rejects("ts-cast", "const x = ({ a: 1 }) satisfies object;");
    rejects("ts-cast", "const a = { b: 1 }; const x = a!;");
    rejects("ts-annotation", "const x: Map<string, number> = null;");
    rejects("ts-annotation", "const x: () => void = null;");
    rejects("ts-annotation", "const x: keyof Foo = null;");
    accepts("const x: { a: number; b?: string[] } = { a: 1 };\nreturn x;");
}

#[test]
fn builtins_and_globals() {
    rejects("ts-date", "const d = new Date();");
    rejects("ts-date", "const d = Date.parse(\"x\");");
    rejects("ts-nondeterministic", "const r = Math.random();");
    rejects("ts-nondeterministic", "setTimeout(() => 1, 5);");
    rejects("ts-nondeterministic", "const t = Date.now();");
    rejects("ts-locale", "const s = (5).toLocaleString();");
    rejects("ts-locale", "const s = \"a\".localeCompare(\"b\");");
    rejects("ts-console", "console.error(\"x\");");
    rejects("ts-unknown-builtin", "const x = [1].foo();");
    rejects("ts-reduce-init", "const x = [1, 2].reduce((a, b) => a + b);");
    rejects("ts-callback-arity", "const x = [1].map((a, b, c) => a);");
    rejects("ts-callback-arity", "const x = [1].reduce((a, b, c) => a, 0);");
    rejects("ts-sort-comparator", "const x = [1, 2].map((v) => v).sort((a, b) => a < b);");
    rejects("ts-sort-comparator", "const x = [\"a\"].map((v) => v).sort((a, b) => a.length - b.size);");
    rejects("ts-json", "const x = JSON.stringify({ a: 1 }, null, \"  \");");
    rejects("ts-json", "const x = JSON.stringify({ a: 1 }, (k, v) => v);");
    rejects("ts-json-parse-type", "const x = JSON.parse(\"{}\");");
    rejects("ts-json-parse-type", "const x = JSON.parse(\"{}\") as unknown;");
    rejects("ts-map-order", "const m = Object.fromEntries([[\"a\", 1]]); const k = Object.keys(m);");
    rejects("ts-mutating-method", "const xs = [1]; xs.pop();");
    rejects("ts-mutating-method", "const o = Object.assign({}, { a: 1 });");
    accepts("const xs = [3, 1].map((v) => v);\nreturn xs.sort((a, b) => a - b);");
}

#[test]
fn tools_and_async() {
    rejects("ts-tools-escape", "const t = tools;");
    rejects("ts-tools-escape", "const t = tools.fs;");
    rejects("ts-tools-escape", "const f = tools[\"fs\"];");
    rejects("unknown-tool", "await tools.fs.readTxt({ path: \"a\" });");
    rejects("unknown-tool", "await tools.nope.read({ });");
    rejects("ts-promise-value", "const p = tools.fs.readText({ path: \"a\" });");
    rejects("ts-promise-value", "const p = Promise.all([tools.fs.readText({ path: \"a\" })]);");
    rejects("ts-promise-value", "const x = await 5;");
    rejects("ts-log-in-parallel", "await Promise.all([\"a\"].map(async (p) => { console.log(p); return await tools.fs.readText({ path: p }); }));");
    accepts("const [a, b] = await Promise.all([tools.fs.readText({ path: \"a\" }), tools.fs.readText({ path: \"b\" })]);\nreturn [a, b];");
}

#[test]
fn functions_and_recursion() {
    rejects("ts-recursion", "function f(n) { return f(n - 1); }\nreturn f(3);");
    rejects("ts-recursion", "function a() { return b(); }\nfunction b() { return a(); }\nreturn a();");
    rejects("ts-function-value", "const f = () => 1; const g = f;");
    rejects("ts-function-value", "const o = { f: () => 1 };");
    rejects("ts-function-value", "function mk() { return () => 1; }");
    rejects("ts-function-value", "const x = await tools.fs.readText({ path: () => 1 });");
    accepts("function add(a, b) { return a + b; }\nreturn add(1, 2);");
}

#[test]
fn mutation_rules() {
    rejects("ts-mutation-pattern", "let a = 0; const b = (a += 1);");
    rejects("ts-mutation-pattern", "const xs = [1]; let i = 0; xs[i++] = 2;");
    rejects("ts-mutation-pattern", "const c = 1; c = 2;");
    rejects("ts-mutation-pattern", "function f(xs) { xs.push(1); return xs; }");
    rejects("ts-mutation-pattern", "let a = 0; let b = 0; [a, b] = [1, 2];");
    rejects("ts-mutation-escape", "let n = 0;\nfunction bump() { return n; }\nn = 1;\nreturn bump();");
    rejects("ts-mutation-escape", "let n = 0;\nconst f = (x) => { n = x; return n; };\nreturn f(1);");
    rejects("ts-mutation-escape", "let n = 0;\n[1].forEach((x) => { n += x; });\nreturn n;");
    rejects("ts-mutation-in-try", "let n = 0;\ntry { n = 1; await tools.fs.readText({ path: \"a\" }); } catch { n = 2; }\nreturn n;");
    rejects("ts-uninitialized", "let t;\nreturn t;");
    rejects("ts-uninitialized", "const t;");
    accepts("let t;\ntry { t = await tools.fs.readText({ path: \"a\" }); } catch { t = \"\"; }\nreturn t;");
    accepts("const o = { a: 1 };\no.b = 2;\nreturn o;");
}

#[test]
fn errors_and_throw() {
    rejects("ts-throw", "throw 5;");
    rejects("ts-throw", "throw new Error(\"a\") + 1;");
    rejects("ts-throw", "throw { message: \"x\" };");
    rejects("ts-error-use", "try { await tools.fs.readText({ path: \"a\" }); throw new Error(\"x\"); } catch (e) { return e.kind; }");
    rejects("ts-error-use", "try { JSON.parse(\"{\") as { a: number }; } catch (e) { return e.message; }");
    rejects("ts-error-use", "try { await tools.fs.readText({ path: \"a\" }); } catch ({ message }) { return message; }");
    rejects("ts-error-use", "try { await tools.fs.readText({ path: \"a\" }); throw new Error(\"x\"); } catch (e) { return e; }");
    accepts("try { await tools.fs.readText({ path: \"a\" }); } catch (e) { return e; }\nreturn 1;");
}

#[test]
fn optional_values() {
    rejects("ts-optional-chain", "const o = { a: { b: 1 } };\nconst x = o?.a;\nreturn x;");
    rejects("ts-optional-chain", "const o = { a: 1 };\nreturn o?.a;");
    rejects("ts-optional-chain", "const o = { a: 1 };\nreturn o?.a === null;");
    accepts("const o = { a: 1 };\nreturn o?.a ?? 0;");
    accepts("const o = { a: 1 };\nif (o?.a) { return 1; }\nreturn 2;");
    accepts("const o = { a: 1 };\nconst x = o?.a;\nreturn x ?? 3;");
}

#[test]
fn bounded_forms() {
    rejects("ts-bound-not-literal", "const n = 5;\nawait sleep(n);");
    rejects("ts-bound-not-literal", "const n = 5;\nawait retry({ delaysMs: [n] }, () => tools.fs.readText({ path: \"a\" }));");
    rejects("ts-bound-not-literal", "await retry({ bogus: 1 }, () => tools.fs.readText({ path: \"a\" }));");
    rejects("ts-bound-not-literal", "await parMap({ limit: 2 + 3 }, [\"a\"], (p) => tools.fs.readText({ path: p }));");
    rejects("ts-promise-value", "const p = retry({ delaysMs: [1] }, () => tools.fs.readText({ path: \"a\" }));");
    accepts("return await retry({ delaysMs: [1000, 2000], on: [\"Timeout\"] }, () => tools.fs.readText({ path: \"a\" }));");
}
