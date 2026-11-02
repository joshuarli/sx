import { expect, test } from "bun:test";
import { classify } from "./classify.mjs";

const tiers = (code) => classify(code).tiers;
const features = (code) => classify(code).features;

test("a script of literal tool calls over finite data is in every tier", () => {
  const code = `
    const repos = await tools.mcp__repomod__catalog_select({repos: [], tags: []});
    const rows = await Promise.all(repos.structuredContent.result.map((r) => tools.mcp__repomod__git_status({repo: r.id})));
    for (const row of rows) console.log(row);
    return rows.filter((row) => row.isError !== true).length;`;
  expect(tiers(code)).toEqual({ callSet: true, asWritten: true, withRewriting: true });
  expect(classify(code).calls.map((c) => [c.name, c.literal])).toEqual([
    ["mcp__repomod__catalog_select", true],
    ["mcp__repomod__git_status", false],
  ]);
});

test("a tool chosen at run time or a tools object that escapes loses the call set", () => {
  for (const code of [
    `const name = "mcp__a__b"; await tools[name]({});`,
    `const t = tools; await t.mcp__a__b({});`,
    `const f = tools.mcp__a__b; await f({});`,
    `for (const k of Object.keys(tools)) console.log(k);`,
    `eval("tools.mcp__a__b({})"); await tools.mcp__a__c({});`,
  ]) {
    expect(tiers(code).callSet).toBe(false);
  }
  expect(tiers(`await tools["mcp__a__b"]({x: 1});`).callSet).toBe(true);
});

test("local mutation and try blocks leave the rewriting tier, unbounded loops and recursion leave both", () => {
  const local = `const out = []; for (const x of [1, 2]) out.push(await tools.mcp__a__b({x})); return out;`;
  expect(tiers(local)).toEqual({ callSet: true, asWritten: false, withRewriting: true });
  expect(features(local)).toContain("mutating-method");
  const caught = `try { await tools.mcp__a__b({}); } catch (e) { return String(e); }`;
  expect(tiers(caught)).toEqual({ callSet: true, asWritten: false, withRewriting: true });
  const polled = `let done = false; while (!done) { done = (await tools.mcp__a__b({})).ok; }`;
  expect(tiers(polled)).toEqual({ callSet: true, asWritten: false, withRewriting: false });
  const recursive = `async function walk(p) { const r = await tools.mcp__a__ls({p}); for (const c of r) await walk(c); } await walk("/");`;
  expect(features(recursive)).toContain("recursion");
  expect(tiers(recursive).withRewriting).toBe(false);
});

test("a script that calls no tool is in no tier and a syntax error is reported", () => {
  expect(classify(`const d = await describeNamespace("repomod"); return d;`).callsTools).toBe(false);
  expect(tiers(`return 1 + 1;`).callSet).toBe(false);
  expect(classify(`const = ;`).parsed).toBe(false);
});

test("a counted for loop is bounded and only a while-style loop is unbounded", () => {
  const counted = `for (let i = 0; i < pages.length; i++) await tools.mcp__a__b({page: i});`;
  expect(features(counted)).toContain("counted-loop");
  expect(tiers(counted)).toEqual({ callSet: true, asWritten: false, withRewriting: true });
  for (const code of [
    `for (let i = 0; i < 10; i++) { i = await tools.mcp__a__b({i}); }`,
    `for (;;) { if ((await tools.mcp__a__b({})).done) break; }`,
    `for (let i = 0; ok; i++) await tools.mcp__a__b({i});`,
  ]) {
    expect(features(code)).toContain("unbounded-loop");
  }
});
