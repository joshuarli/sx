import { describe, expect, test } from "bun:test";
import { runProgram } from "./shim.ts";
import type { Entry, ToolSpec } from "./runtime.ts";

const specs: ToolSpec[] = [
  { id: "store.get", effect: "read" },
  { id: "store.put", effect: "write" },
  { id: "mcp-repomod.catalog-select", effect: "read" },
];

const ok = (output: unknown) => ({ output }) as never;
const fail = (kind: string, message: string) => ({ error: { kind, message } }) as never;
const entry = (capability: string, input: unknown, ...results: unknown[]): Entry =>
  ({ capability, input, results }) as Entry;

const run = (source: string, entries: Entry[] = []) => runProgram(source, specs, entries);

describe("tool API", () => {
  test("namespaces and names are the camel case of the kebab case, and calls are recorded in order", async () => {
    const out = await run(
      `const a = await tools.store.get({ id: "a" });
       const b = await tools.mcpRepomod.catalogSelect({ repos: [] });
       return [a, b];`,
      [entry("store.get", { id: "a" }, ok(1)), entry("mcp-repomod.catalog-select", { repos: [] }, ok("x"))],
    );
    expect(out.result).toEqual([1, "x"]);
    expect(out.calls.map((c) => c.capability)).toEqual(["store.get", "mcp-repomod.catalog-select"]);
  });

  test("null input fields are left out of the call, and an omitted input is {}", async () => {
    const out = await run(`await tools.store.get({ id: "a", extra: null }); await tools.store.get();`, [
      entry("store.get", { id: "a" }, ok(1)),
      entry("store.get", {}, ok(2)),
    ]);
    expect(out.error).toBeUndefined();
    expect(out.calls.map((c) => c.input)).toEqual([{ id: "a" }, {}]);
  });

  test("a read answers repeatedly with its last result, and a write answers once per result", async () => {
    const reads = await run(`return [await tools.store.get({}), await tools.store.get({}), await tools.store.get({})];`, [
      entry("store.get", {}, ok(1), ok(2)),
    ]);
    expect(reads.result).toEqual([1, 2, 2]);
    const writes = await run(`await tools.store.put({}); await tools.store.put({});`, [entry("store.put", {}, ok(null))]);
    expect(writes.error?.class).toBe("unexpected-call");
    expect(writes.calls).toHaveLength(2);
  });

  test("a call with no recorded response ends the run and a catch does not see it", async () => {
    const out = await run(`try { await tools.store.get({ id: "z" }); } catch { return "caught"; } return "no";`);
    expect(out.error?.class).toBe("unexpected-call");
  });

  test("each call returns a fresh value", async () => {
    const out = await run(
      `const a = await tools.store.get({}); a.push(9); const b = await tools.store.get({}); return b;`,
      [entry("store.get", {}, ok([1]))],
    );
    expect(out.result).toEqual([1]);
  });
});

describe("call errors", () => {
  test("a failed call throws a CallError that JSON.stringify writes as kind then message", async () => {
    const out = await run(
      `try { await tools.store.get({}); } catch (e) { return [JSON.stringify(e), String(e), e.kind, e.message, Object.keys(e)]; }`,
      [entry("store.get", {}, fail("NotFound", "gone"))],
    );
    expect(out.result).toEqual(['{"kind":"NotFound","message":"gone"}', "CallError: gone", "NotFound", "gone", ["kind", "message"]]);
  });

  test("an uncaught CallError is the failure of the run", async () => {
    const out = await run(`await tools.store.get({});`, [entry("store.get", {}, fail("Timeout", "slow"))]);
    expect(out.error).toEqual({ class: "call-failed", kind: "Timeout", message: "slow" });
  });

  test("an uncaught throw new Error is thrown, with its message", async () => {
    const out = await run(`throw new Error("boom");`);
    expect(out.error).toEqual({ class: "thrown", message: "boom" });
  });
});

describe("console.log", () => {
  test("strings are themselves, numbers and booleans their text, null is null, objects are JSON", async () => {
    const out = await run(`console.log("a", 1, 1.0, 2.5, true, null, { x: [1, "b"] }); console.log(); return null;`);
    expect(out.logs).toEqual(['a 1 1 2.5 true null {"x":[1,"b"]}', ""]);
  });

  test("a failed run keeps the logs up to the failure", async () => {
    const out = await run(`console.log("one"); await tools.store.get({}); console.log("two");`, [
      entry("store.get", {}, fail("Failed", "x")),
    ]);
    expect(out.logs).toEqual(["one"]);
  });
});

describe("traps", () => {
  const trapOf = async (source: string) => {
    const out = await run(source);
    return out.error?.class === "trap" ? out.error.trap : out.error ? out.error.class : "completed";
  };

  test("an Int result beyond 2^53 is the arithmetic trap, and 2^53 itself is not", async () => {
    expect(await trapOf(`let n = 9007199254740991; n = n + 2; return n;`)).toBe("arithmetic");
    expect(await trapOf(`return 4503599627370496 * 4;`)).toBe("arithmetic");
    expect(await trapOf(`return 4503599627370496 * 2;`)).toBe("completed");
    expect(await trapOf(`return 0.1 + 0.2;`)).toBe("completed");
  });

  test("division and remainder by zero are the arithmetic trap", async () => {
    expect(await trapOf(`return 1 / 0;`)).toBe("arithmetic");
    expect(await trapOf(`const z = 0; return 5 % z;`)).toBe("arithmetic");
    expect(await trapOf(`return -7 % 3;`)).toBe("completed");
  });

  test("compound assignment is checked like the operator", async () => {
    expect(await trapOf(`let n = 9007199254740991; n += 2; return n;`)).toBe("arithmetic");
  });

  test("an index past the end, and a destructuring past the end, are the out-of-range trap", async () => {
    expect(await trapOf(`const xs = [1, 2]; return xs[2];`)).toBe("out-of-range");
    expect(await trapOf(`const [a, b, c] = [1, 2]; return c;`)).toBe("out-of-range");
    expect(await trapOf(`for (const [a, b] of [[1]]) { return b; }`)).toBe("out-of-range");
    expect(await trapOf(`const f = ([a, b]) => b; return f([1]);`)).toBe("out-of-range");
    expect(await trapOf(`const xs = [1, 2]; return xs[1];`)).toBe("completed");
  });

  test("a read of an absent map key is undefined, which a result cannot hold", async () => {
    expect(await trapOf(`const m = { a: 1 }; const k = "b"; return m[k] ?? 0;`)).toBe("completed");
    expect(await trapOf(`const m = { a: 1 }; const k = "b"; return { v: m[k] };`)).toBe("out-of-range");
  });

  test("parseInt without digits is the arithmetic trap", async () => {
    expect(await trapOf(`return parseInt("12px", 10);`)).toBe("completed");
    expect(await trapOf(`return parseInt("px", 10);`)).toBe("arithmetic");
  });

  test("rounding a value outside the Int range is the out-of-range trap", async () => {
    expect(await trapOf(`return Math.floor(2.5e20);`)).toBe("out-of-range");
    expect(await trapOf(`return Math.round(2.5);`)).toBe("completed");
  });

  test("a slice or index that cuts a surrogate pair is the arithmetic trap", async () => {
    expect(await trapOf(`return "a😀b".slice(0, 2);`)).toBe("arithmetic");
    expect(await trapOf(`return "a😀b".slice(0, 3);`)).toBe("completed");
    expect(await trapOf(`const s = "a😀b"; return s[2];`)).toBe("arithmetic");
  });

  test("the value of a rejected Settled is the out-of-range trap and its fields are not", async () => {
    const entries = [entry("store.get", { id: "x" }, fail("NotFound", "gone"))];
    const read = await run(`const [s] = await Promise.allSettled([tools.store.get({ id: "x" })]); return s.value;`, entries);
    expect(read.error).toMatchObject({ class: "trap", trap: "out-of-range" });
    const whole = await run(`const [s] = await Promise.allSettled([tools.store.get({ id: "x" })]); return [s.status, s.reason.kind, s];`, entries);
    expect(whole.result).toEqual(["rejected", "NotFound", { status: "rejected", reason: { kind: "NotFound", message: "gone" } }]);
  });

  test("a trap is not seen by catch, retry, or allSettled", async () => {
    expect(await trapOf(`try { const xs = [1]; return xs[3]; } catch { return "caught"; }`)).toBe("out-of-range");
    expect(await trapOf(`return await retry({ delaysMs: [1], on: ["Failed"] }, async () => { const xs = [1]; return xs[3]; });`)).toBe("out-of-range");
    expect(await trapOf(`const [r] = await Promise.allSettled([(async () => { const xs = [1]; return xs[3]; })()]); return r.status;`)).toBe("out-of-range");
  });

  test("undefined, NaN, and an unsafe integer in the result are traps by their class", async () => {
    expect(await trapOf(`return [1, undefined];`)).toBe("out-of-range");
    expect(await trapOf(`return Math.max();`)).toBe("arithmetic");
    expect(await trapOf(`return 2 ** 60;`)).toBe("arithmetic");
  });
});

describe("bounded forms", () => {
  const timeouts = (n: number, then: unknown) => [...Array.from({ length: n }, () => fail("Timeout", "slow")), ok(then)];

  test("retry repeats a transient failure with the listed delays and then succeeds", async () => {
    const out = await run(`return await retry({ delaysMs: [1000, 2000] }, () => tools.store.get({}));`, [
      entry("store.get", {}, ...timeouts(2, "v")),
    ]);
    expect(out.result).toBe("v");
    expect(out.delays).toEqual([1000, 2000]);
    expect(out.calls).toHaveLength(3);
  });

  test("retry gives up with the last CallError when delays run out, and does not retry other kinds", async () => {
    const out = await run(`return await retry({ delaysMs: [10] }, () => tools.store.get({}));`, [entry("store.get", {}, ...timeouts(3, "v"))]);
    expect(out.error).toMatchObject({ class: "call-failed", kind: "Timeout" });
    expect(out.calls).toHaveLength(2);
    const missing = await run(`return await retry({ delaysMs: [10, 10] }, () => tools.store.get({}));`, [entry("store.get", {}, fail("NotFound", "x"))]);
    expect(missing.delays).toEqual([]);
    expect(missing.calls).toHaveLength(1);
    const named = await run(`return await retry({ delaysMs: [10, 10], on: ["NotFound"] }, () => tools.store.get({}));`, [
      entry("store.get", {}, fail("NotFound", "x"), ok(5)),
    ]);
    expect(named.result).toBe(5);
  });

  test("retry with a backoff doubles the delay up to the cap while the total stays within the limit", async () => {
    const out = await run(`return await retry({ backoffMs: [100, 300], withinMs: 700 }, () => tools.store.get({}));`, [
      entry("store.get", {}, ...timeouts(9, "v")),
    ]);
    expect(out.delays).toEqual([100, 200, 300]);
    expect(out.error).toMatchObject({ class: "call-failed", kind: "Timeout" });
  });

  test("waitUntil returns the first value that satisfies the predicate and records the waits", async () => {
    const out = await run(`return await waitUntil({ everyMs: 500, withinMs: 2000 }, () => tools.store.get({}), (v) => v === 3);`, [
      entry("store.get", {}, ok(1), ok(2), ok(3)),
    ]);
    expect(out.result).toBe(3);
    expect(out.delays).toEqual([500, 500]);
  });

  test("waitUntil fails with a Timeout CallError when the time runs out, which catch sees", async () => {
    const out = await run(
      `try { await waitUntil({ everyMs: 500, withinMs: 1000 }, () => tools.store.get({}), (v) => v === 9); } catch (e) { return e.kind; }`,
      [entry("store.get", {}, ok(1))],
    );
    expect(out.result).toBe("Timeout");
    expect(out.delays).toEqual([500, 500]);
  });

  test("sleep records its duration and a zero sleep records nothing", async () => {
    const out = await run(`await sleep(250); await sleep(0); await sleep(1000);`);
    expect(out.delays).toEqual([250, 1000]);
  });

  test("loop carries its state and ends with done; reaching max with next is the loop-limit trap", async () => {
    const paged = await run(
      `return await loop({ max: 5 }, { cursor: 0, seen: [] }, ({ cursor, seen }) => {
         if (cursor === 3) return done(seen);
         return next({ cursor: cursor + 1, seen: [...seen, cursor] });
       });`,
    );
    expect(paged.result).toEqual([0, 1, 2]);
    const limited = await run(`return await loop({ max: 2 }, { n: 0 }, ({ n }) => next({ n: n + 1 }));`);
    expect(limited.error).toMatchObject({ class: "trap", trap: "loop-limit" });
  });

  test("parMap returns results in order and fails with the lowest-numbered failure", async () => {
    const entries = [
      entry("store.get", { id: "a" }, ok("A")),
      entry("store.get", { id: "b" }, fail("NotFound", "b")),
      entry("store.get", { id: "c" }, fail("Invalid", "c")),
    ];
    const good = await run(`return await parMap({ limit: 2 }, ["a", "a"], (id) => tools.store.get({ id }));`, entries);
    expect(good.result).toEqual(["A", "A"]);
    const bad = await run(`return await parMap({ limit: 3 }, ["a", "c", "b"], (id) => tools.store.get({ id }));`, entries);
    expect(bad.error).toMatchObject({ class: "call-failed", kind: "Invalid" });
  });

  test("Promise.all fails with the lowest-numbered failed branch even when a later one fails first", async () => {
    const entries = [
      entry("store.get", { id: "slow" }, fail("Timeout", "slow")),
      entry("store.get", { id: "fast" }, fail("Conflict", "fast")),
    ];
    const out = await run(
      `await Promise.all([(async () => { await sleep(1); await tools.store.get({ id: "slow" }); })(), tools.store.get({ id: "fast" })]);`,
      entries,
    );
    expect(out.error).toMatchObject({ class: "call-failed", kind: "Timeout" });
  });

  test("a branch with no recorded response is reported before any other branch's failure", async () => {
    const entries = [entry("store.get", { id: "a" }, fail("Timeout", "a"))];
    const all = await run(`await Promise.all([tools.store.get({ id: "a" }), tools.store.get({ id: "b" })]);`, entries);
    expect(all.error?.class).toBe("unexpected-call");
    const mapped = await run(`await parMap({ limit: 2 }, ["a", "b"], (id) => tools.store.get({ id }));`, entries);
    expect(mapped.error?.class).toBe("unexpected-call");
  });
});

describe("language", () => {
  test("an arrow with an array pattern and a parenthesized object body keeps its body", async () => {
    const out = await run(`return await Promise.all([["a", "b"]].map(async([x, y]) => ({ x, y })));`);
    expect(out.error).toBeUndefined();
    expect(out.result).toEqual([{ x: "a", y: "b" }]);
  });

  test("type annotations are erased and the body may use top-level await and return", async () => {
    const out = await run(`type T = { a: number }; const x: T = { a: 2 }; const f = (n: number): number => n * 2; return f(x.a);`);
    expect(out.result).toBe(4);
  });

  test("a program that returns nothing returns null", async () => {
    const out = await run(`const x = 1;`);
    expect(out.result).toBeNull();
    expect(out.error).toBeUndefined();
  });
});
