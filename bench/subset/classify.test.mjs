import { expect, test } from "bun:test";
import { classify, rewriteApi } from "./classify.mjs";

const codesOf = (code) => Object.keys(classify(code).codes ?? {}).sort();
const accepted = (code) => classify(code).tiers.accepted;
const call = `await tools.fs.readText({path: "/a"})`;

test("the API rewrite maps codemode's results and names onto the subset's", () => {
  expect(rewriteApi(`const r = await tools.mcp__tools__fs_read_text({path: "/a"}); return r.content[0].text;`))
    .toContain(`const r = await tools.fs.readText({path: "/a"}); return r;`);
  expect(rewriteApi(`const r = await tools.mcp__repomod__git_status({repo: "x"}); return r.structuredContent?.result ?? [];`))
    .toContain(`return r ?? [];`);
  expect(rewriteApi(`const r = await tools.mcp__a__b_c({}); if (r.isError) return 1; return r.structuredContent.id;`))
    .toContain(`if (false) return 1; return r.id;`);
  expect(rewriteApi(`text(JSON.stringify(1)); function text() {}`)).toContain(`text(JSON.stringify(1))`);
  expect(rewriteApi(`text("x"); const r = await tools.read({path: "/a"});`)).toContain(`console.log("x"); const r = await tools.pi.read(`);
  expect(rewriteApi(`const r = await tools.mcp__a__b({}); return r.structuredContent ?? r.content;`)).toContain(`return r;`);
  expect(rewriteApi(`const r = await tools.mcp__a__b({}); return r.content.map((c) => c.text).join("");`)).toContain(`return r;`);
});

test("a script of literal tool calls over finite data is accepted without rewriting", () => {
  const code = `
    const repos = await tools.mcp__repomod__catalog_select({repos: [], tags: []});
    const rows = await Promise.all(repos.structuredContent.result.map((r) => tools.mcp__repomod__git_status({repo: r.id})));
    for (const row of rows) console.log(row);
    return rows.length;`;
  const result = classify(code);
  expect(result.tiers).toEqual({ callSet: true, asWritten: true, accepted: true });
  expect(result.calls.map((c) => [c.name, c.literal])).toEqual([
    ["catalog.select", true],
    ["git.status", false],
  ]);
});

test("a script that calls no tool has no tier and a syntax error is reported", () => {
  expect(classify(`const d = await describeNamespace("repomod"); return d;`).callsTools).toBe(false);
  expect(classify(`return 1 + 1;`).callsTools).toBe(false);
  expect(classify(`const = ;`).parsed).toBe(false);
});

test("a tool chosen at run time, a tools object that escapes, and eval lose the call set", () => {
  for (const code of [
    `const name = "a"; await tools[name]({});`,
    `const t = tools; await t.fs.readText({});`,
    `const f = tools.fs.readText; await f({});`,
    `for (const k of Object.keys(tools)) console.log(k);`,
    `eval("1"); ${call};`,
    `const names = ALL_TOOLS; ${call};`,
  ]) {
    expect(classify(code).tiers.callSet).toBe(false);
  }
  expect(accepted(`await tools["fs"].readText({path: "/a"});`)).toBe(false);
  expect(accepted(`${call};`)).toBe(true);
});

test("loops are for-of or counted; every other loop is rejected", () => {
  expect(codesOf(`while (true) { ${call}; break; }`)).toEqual(["ts-while"]);
  expect(codesOf(`do { ${call}; } while (false);`)).toEqual(["ts-while"]);
  expect(codesOf(`for (;;) { ${call}; break; }`)).toEqual(["ts-while"]);
  expect(codesOf(`for (const k in {a: 1}) ${call};`)).toEqual(["ts-for-in"]);
  expect(accepted(`for (let i = 0; i < 3; i++) ${call};`)).toBe(true);
  expect(accepted(`for (let i = 5; i >= 0; i -= 2) ${call};`)).toBe(true);
  const xs = `const xs = await tools.fs.list({path: "/"});`;
  expect(accepted(`${xs} for (let i = 0; i < xs.length; i++) ${call};`)).toBe(true);
  expect(codesOf(`${xs} for (let i = 0; i < xs.length; i++) { xs.push(${call}); }`)).toContain("ts-counted-loop");
  expect(codesOf(`for (let i = 0; i < 3; i++) { i += 1; ${call}; }`)).toEqual(["ts-counted-loop"]);
  expect(codesOf(`for (let i = 0; i < 3; i += 0.5) ${call};`)).toEqual(["ts-counted-loop"]);
  expect(codesOf(`for (let i = 0; i > 3; i++) ${call};`)).toEqual(["ts-counted-loop"]);
  expect(codesOf(`for (let i = 0; i < f(); i++) ${call}; function f() { return 3; }`)).toEqual(["ts-counted-loop"]);
});

test("statements and forms outside the grammar are rejected with their own codes", () => {
  expect(codesOf(`switch (1) { case 1: ${call}; }`)).toEqual(["ts-switch"]);
  expect(codesOf(`outer: for (const x of [1]) { ${call}; break outer; }`)).toEqual(["ts-label"]);
  expect(codesOf(`try { ${call}; } catch (e) { } finally { }`)).toEqual(["ts-finally"]);
  expect(codesOf(`class A {} ${call};`)).toEqual(["ts-class"]);
  expect(codesOf(`function* g() { yield 1; } ${call};`)).toEqual(["ts-generator"]);
  expect(codesOf(`const x = 2 ** 3; ${call};`)).toEqual(["ts-operator"]);
  expect(codesOf(`const x = 5 & 3; ${call};`)).toEqual(["ts-operator"]);
  expect(codesOf(`var x = 1; ${call};`)).toEqual(["ts-unsupported"]);
  expect(codesOf(`const x = (1, 2); ${call};`)).toEqual(["ts-operator"]);
  expect(codesOf(`import fs from "fs"; ${call};`)).toContain("ts-import-export");
});

test("recursion is rejected, a call between declared functions is not", () => {
  expect(codesOf(`async function walk(p) { const r = ${call}; for (const c of r) await walk(c); } await walk("/");`)).toEqual(["ts-recursion"]);
  expect(accepted(`async function read(p) { return await tools.fs.readText({path: p}); } await read("/a");`)).toBe(true);
  expect(codesOf(`const f = async (n) => { return n ? await f(n - 1) : 0; }; ${call};`)).toEqual(["ts-recursion"]);
});

test("equality, undefined, typeof and instanceof follow the null-only model", () => {
  expect(codesOf(`const x = 1; if (x == 2) ${call};`)).toEqual(["ts-loose-equality"]);
  expect(accepted(`const x = null; if (x == null) ${call};`)).toBe(true);
  expect(codesOf(`let x = undefined; ${call};`)).toEqual(["ts-undefined"]);
  expect(codesOf(`const x = 1; if (x === undefined) ${call};`)).toEqual(["ts-undefined"]);
  expect(codesOf(`const x = 1; if (typeof x === "undefined") ${call};`)).toEqual(["ts-undefined"]);
  expect(accepted(`const x = "a"; if (typeof x === "string") ${call};`)).toBe(true);
  expect(codesOf(`const x = {}; if (x instanceof Error) ${call};`)).toEqual(["ts-instanceof"]);
  expect(accepted(`const x = []; if (Array.isArray(x)) ${call};`)).toBe(true);
});

test("an optional chain may be defaulted, compared with null or a literal, or tested, but not used as a value", () => {
  const r = `const r = ${call};`;
  expect(accepted(`${r} const y = r?.length ?? 0; ${call};`)).toBe(true);
  expect(accepted(`${r} if (r?.length != null) ${call};`)).toBe(true);
  expect(accepted(`${r} if (r?.kind === "Directory") ${call};`)).toBe(true);
  expect(accepted(`${r} if (r?.length) ${call};`)).toBe(true);
  expect(accepted(`${r} if (r?.length && r.length > 2) ${call};`)).toBe(true);
  expect(codesOf(`${r} console.log(r?.length);`)).toEqual(["ts-optional-chain"]);
  expect(codesOf(`${r} const y = r?.length; console.log(y);`)).toEqual(["ts-optional-chain"]);
  expect(accepted(`${r} const y = r?.length; const z = y ?? 0; ${call};`)).toBe(true);
  expect(codesOf(`${r} if (r?.length === null) ${call};`)).toEqual(["ts-optional-chain"]);
  expect(codesOf(`${r} const m = "x" + r?.length;`)).toEqual(["ts-optional-chain"]);
  expect(accepted(`${r} await tools.fs.readText({path: r?.length});`)).toBe(true);
});

test("local mutation is accepted in the listed patterns and rejected elsewhere", () => {
  const sum = `let total = 0; for (const x of [1, 2]) { total += x; } return total; ${call};`;
  expect(classify(sum).tiers).toEqual({ callSet: true, asWritten: false, accepted: true });
  expect(accepted(`const out = []; for (const x of [1, 2]) { out.push(await tools.fs.readText({path: "/a"})); } return out;`)).toBe(true);
  expect(accepted(`const m = {}; for (const x of ["a"]) { m[x] = 1; } m.total = 2; return m; ${call};`)).toBe(true);
  expect(accepted(`const names = ["b", "a"]; names.sort(); return names; ${call};`)).toBe(true);
  expect(codesOf(`let n = 0; const xs = [1]; const y = xs[n++]; ${call};`)).toContain("ts-mutation-pattern");
  expect(codesOf(`const xs = [1, 2]; xs.pop(); ${call};`)).toEqual(["ts-mutating-method"]);
  expect(accepted(`const o = ${call}; o.count = 1; return o;`)).toBe(true);
  expect(codesOf(`function f(o) { o.count = 1; } f({}); ${call};`)).toContain("ts-mutation-pattern");
});

test("a mutated variable may not be captured or aliased", () => {
  expect(codesOf(`let n = 0; [1, 2].forEach((x) => { n += x; }); ${call};`)).toEqual(["ts-mutation-escape"]);
  expect(accepted(`let n = 0; for (const x of [1]) { n += x; } const ys = [1].map((x) => x + n); ${call};`)).toBe(true);
  expect(codesOf(`let n = 0; const f = (x) => x + n; n += 1; ${call}; return f(1);`)).toEqual(["ts-mutation-escape"]);
  expect(codesOf(`const a = []; const b = {items: a}; a.push(1); ${call};`)).toEqual(["ts-mutation-escape"]);
  expect(accepted(`const a = []; a.push(1); const b = {items: a}; ${call};`)).toBe(true);
  expect(codesOf(`const a = []; for (const x of [1]) { const b = {items: a}; a.push(x); } ${call};`)).toEqual(["ts-mutation-escape"]);
  expect(codesOf(`const a = []; const b = a; b.push(1); ${call};`)).toEqual(["ts-mutation-pattern"]);
});

test("an assignment in a try must come after the last operation that can throw", () => {
  expect(accepted(`let ok = false; try { ${call}; ok = true; } catch (e) { } return ok;`)).toBe(true);
  expect(accepted(`let r = 0; try { r = (${call}).length; } catch (e) { r = -1; } return r;`)).toBe(true);
  expect(codesOf(`let n = 0; try { n += 1; ${call}; } catch (e) { console.log(n); }`)).toEqual(["ts-mutation-in-try"]);
  expect(codesOf(`const out = []; try { for (const x of [1, 2]) { out.push(x); ${call}; } } catch (e) { }`)).toEqual(["ts-mutation-in-try"]);
});

test("a catch parameter is restricted when the try block can throw a plain error", () => {
  expect(accepted(`try { ${call}; } catch (e) { console.log(e.message, e.kind, String(e)); }`)).toBe(true);
  expect(accepted(`try { ${call}; } catch (e) { return {error: e}; }`)).toBe(true);
  expect(accepted(`try { ${call}; throw new Error("x"); } catch (e) { console.log(e.message); }`)).toBe(true);
  expect(codesOf(`try { ${call}; throw new Error("x"); } catch (e) { console.log(e.kind); }`)).toEqual(["ts-error-use"]);
  expect(codesOf(`try { ${call}; throw new Error("x"); } catch (e) { console.log(String(e)); }`)).toEqual(["ts-error-use"]);
  expect(codesOf(`try { ${call}; throw new Error("x"); } catch (e) { return e; }`)).toEqual(["ts-error-use"]);
  expect(codesOf(`try { const x = JSON.parse(${call}); } catch (e) { console.log(e.message); }`)).toContain("ts-error-use");
  expect(codesOf(`try { ${call}; } catch (e) { console.log(e.stack); }`)).toEqual(["ts-error-use"]);
  expect(accepted(`try { ${call}; } catch (e) { throw e; }`)).toBe(true);
  expect(accepted(`try { ${call}; } catch (e) { if (e && e.message) console.log(e.message); }`)).toBe(true);
  expect(accepted(`try { ${call}; } catch { }`)).toBe(true);
});

test("throw accepts new Error with a message and the caught error only", () => {
  expect(accepted(`if (${call}) throw new Error("bad");`)).toBe(true);
  expect(codesOf(`throw "bad"; ${call};`)).toEqual(["ts-throw"]);
  expect(codesOf(`throw new Error("a", {cause: 1}); ${call};`)).toEqual(["ts-throw"]);
  expect(codesOf(`const e = new Error("a"); ${call};`)).toEqual(["ts-class"]);
});

test("allSettled results are guarded by status, and may be printed only when no branch throws a plain error", () => {
  const settled = `const [a, b] = await Promise.allSettled([tools.fs.readText({path: "/a"}), tools.fs.list({path: "/"})]);`;
  expect(accepted(`${settled} const t = a.status === "fulfilled" ? a.value : "";`)).toBe(true);
  expect(accepted(`${settled} console.log(JSON.stringify(a));`)).toBe(true);
  const risky = `const rs = await Promise.allSettled([(async () => { const t = await tools.fs.readText({path: "/a"}); if (!t) throw new Error("x"); return t; })()]);`;
  expect(codesOf(`${risky} console.log(JSON.stringify(rs[0]));`)).toContain("ts-settled-escape");
  expect(accepted(`${settled} console.log({i: 0, ...a});`)).toBe(true);
  const many = `const rs = await Promise.allSettled(["/a"].map((p) => tools.fs.readText({path: p})));`;
  expect(accepted(`${many} for (const r of rs) { console.log(r.status === "fulfilled" ? r.value : r.reason?.message); }`)).toBe(false);
  expect(accepted(`${many} for (const r of rs) { console.log(r.status === "fulfilled" ? r.value : (r.reason?.message ?? "")); }`)).toBe(true);
  expect(accepted(`${many} console.log(JSON.stringify(rs));`)).toBe(true);
});

test("promises are awaited where they are made, or collected by Promise.all", () => {
  expect(accepted(`const [x, y] = await Promise.all([${call}, tools.fs.list({path: "/"})]);`)).toBe(true);
  expect(accepted(`await Promise.all(["/a"].map(async (p) => { const t = await tools.fs.readText({path: p}); return t; }));`)).toBe(true);
  expect(codesOf(`const p = tools.fs.readText({path: "/a"}); await p;`)).toContain("ts-promise-value");
  expect(codesOf(`const p = ${call}; const q = await p;`)).toContain("ts-promise-value");
  expect(codesOf(`await Promise.all(["/a"].map(async (p) => { console.log(p); return ${call}; }));`)).toEqual(["ts-log-in-parallel"]);
  expect(codesOf(`await (async () => ${call})();`)).toEqual(["ts-function-value"]);
});

test("functions are called or passed to builtins, never stored or returned", () => {
  expect(codesOf(`const make = () => () => 1; ${call};`)).toEqual(["ts-function-value"]);
  expect(codesOf(`const o = {f: () => 1}; ${call};`)).toEqual(["ts-function-value"]);
  expect(codesOf(`const f = (a = 1) => a; ${call};`)).toEqual(["ts-param"]);
  expect(codesOf(`const f = (...a) => a; ${call};`)).toEqual(["ts-param"]);
  expect(accepted(`const f = ({a, b}, [c, d]) => a + b + c + d; ${call};`)).toBe(true);
});

test("object literals have static, non-integer keys and no spread", () => {
  expect(codesOf(`const o = {[call]: 1}; ${call};`)).toContain("ts-key");
  expect(codesOf(`const o = {1: "a"}; ${call};`)).toEqual(["ts-key"]);
  expect(codesOf(`const o = {a: 1, a: 2}; ${call};`)).toEqual(["ts-key"]);
  expect(accepted(`const o = {a: 1}; const p = {...o, b: 2}; const q = {...o, ...p.inner, ...[p][0]}; ${call};`)).toBe(true);
  expect(codesOf(`const o = {a: 1}; const p = {...(o || {}), b: 2}; ${call};`)).toEqual(["ts-spread"]);
  expect(codesOf(`const p = {...(await tools.fs.stat({path: "/a"}))}; ${call};`)).toEqual(["ts-spread"]);
  expect(codesOf(`const o = {a: 1}; const p = {...o?.b}; ${call};`)).toContain("ts-spread");
  expect(codesOf(`const p = {...{a: 1}}; ${call};`)).toEqual(["ts-spread"]);
  expect(accepted(`const a = [1]; const b = [...a, 2]; ${call};`)).toBe(true);
  expect(codesOf(`Math.max(...[1, 2]); ${call};`)).toEqual(["ts-spread"]);
  expect(codesOf(`const o = {get a() { return 1; }}; ${call};`)).toContain("ts-class");
});

test("numbers beyond the safe range are rejected as literals", () => {
  expect(codesOf(`const n = 9007199254740993; ${call};`)).toEqual(["ts-unsafe-integer"]);
  expect(accepted(`const n = 9007199254740992; const f = 1e300; ${call};`)).toBe(true);
});

test("builtins outside the table, regular expressions, dates, and nondeterminism are rejected", () => {
  expect(codesOf(`[1].at(0); ${call};`)).toEqual(["ts-unknown-builtin"]);
  expect(codesOf(`store("k", 1); ${call};`)).toEqual(["ts-codemode-store"]);
  expect(codesOf(`const v = load("k"); ${call};`)).toEqual(["ts-codemode-store"]);
  expect(accepted(`function load(k) { return k; } load("k"); ${call};`)).toBe(true);
  expect(codesOf(`const c = new Set(); ${call};`)).toEqual(["ts-unsupported-class"]);
  expect(codesOf(`"a".match(/a/); ${call};`)).toContain("ts-regexp");
  expect(codesOf(`const r = /a/; ${call};`)).toEqual(["ts-regexp"]);
  expect(codesOf(`new Date(0).toISOString(); ${call};`)).toEqual(["ts-date"]);
  expect(codesOf(`Date.now(); ${call};`)).toEqual(["ts-nondeterministic"]);
  expect(codesOf(`Math.random(); ${call};`)).toEqual(["ts-nondeterministic"]);
  expect(codesOf(`setTimeout(() => 1, 5); ${call};`)).toContain("ts-nondeterministic");
  expect(codesOf(`"a".localeCompare("b"); ${call};`)).toEqual(["ts-locale"]);
  expect(codesOf(`console.error("a"); ${call};`)).toEqual(["ts-console"]);
  expect(codesOf(`parseInt("1"); ${call};`)).toEqual(["ts-unknown-builtin"]);
  expect(accepted(`parseInt("1", 10); Math.floor(1.5); Number.isInteger(2); String(1); "a".padStart(2, "0"); ${call};`)).toBe(true);
  expect(codesOf(`[1].reduce((a, b) => a + b); ${call};`)).toEqual(["ts-reduce-init"]);
  expect(codesOf(`[1].map((a, b, c) => a); ${call};`)).toEqual(["ts-callback-arity"]);
  expect(accepted(`[1].map((a, i) => a + i).filter((a) => a).flatMap((a) => [a]); ${call};`)).toBe(true);
});

test("object enumeration is allowed only where its order cannot be seen", () => {
  expect(codesOf(`Object.entries({a: 1}); ${call};`)).toEqual(["ts-map-order"]);
  expect(codesOf(`Object.keys({a: 1}); ${call};`)).toEqual(["ts-map-order"]);
  expect(accepted(`const ks = Object.keys({a: 1}).sort(); ${call};`)).toBe(true);
  expect(accepted(`const n = Object.keys({a: 1}).length; ${call};`)).toBe(true);
  expect(codesOf(`Object.assign({}, {a: 1}); ${call};`)).toEqual(["ts-mutating-method"]);
});

test("sort comparators must be a difference of one key", () => {
  expect(accepted(`[{a: 1}].sort((x, y) => x.a - y.a); ${call};`)).toBe(true);
  expect(accepted(`[{a: 1}].sort((x, y) => { return y.a - x.a; }); ${call};`)).toBe(true);
  expect(codesOf(`[{a: 1}].sort((x, y) => x.a < y.a ? -1 : 1); ${call};`)).toEqual(["ts-sort-comparator"]);
  expect(codesOf(`[{a: 1}].sort((x, y) => x.a - y.b); ${call};`)).toEqual(["ts-sort-comparator"]);
  expect(codesOf(`["a"].sort((x, y) => x.localeCompare(y)); ${call};`)).toEqual(["ts-locale"]);
});

test("JSON.stringify takes a value and an optional indent, and JSON.parse needs a declared type", () => {
  expect(accepted(`JSON.stringify({a: 1}); JSON.stringify({a: 1}, null, 2); ${call};`)).toBe(true);
  expect(codesOf(`JSON.stringify({a: 1}, null, "  "); ${call};`)).toEqual(["ts-json"]);
  expect(codesOf(`JSON.stringify({a: 1}, (k, v) => v); ${call};`)).toContain("ts-json");
  expect(codesOf(`JSON.parse(${call}); `)).toEqual(["ts-json-parse-type"]);
});

test("a declaration without an initializer needs every path to assign before use", () => {
  expect(accepted(`let t; try { t = ${call}; } catch (e) { t = ""; } return t;`)).toBe(true);
  expect(accepted(`let t; if (${call}) { t = "a"; } else { t = "b"; } return t;`)).toBe(true);
  expect(accepted(`let t; try { t = ${call}; } catch (e) { return null; } return t;`)).toBe(true);
  expect(codesOf(`let t; if (${call}) { t = "a"; } return t;`)).toEqual(["ts-uninitialized"]);
  expect(codesOf(`let t; for (const x of [1]) { t = x; } return t; ${call};`)).toEqual(["ts-uninitialized"]);
});

test("bounded builtins take literal options", () => {
  expect(accepted(`await retry({delaysMs: [100, 200], on: ["Timeout"]}, () => tools.fs.readText({path: "/a"}));`)).toBe(true);
  expect(codesOf(`const d = 100; await retry({delaysMs: [d]}, () => tools.fs.readText({path: "/a"}));`)).toEqual(["ts-bound-not-literal"]);
  expect(accepted(`await sleep(100); await parMap({limit: 4}, [1], (x) => tools.fs.readText({path: "/a"}));`)).toBe(true);
});
