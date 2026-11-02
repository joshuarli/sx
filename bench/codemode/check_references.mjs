import { readdirSync, readFileSync, existsSync } from "node:fs";
import { join } from "node:path";
import { pathToFileURL } from "node:url";
import { isDeepStrictEqual } from "node:util";
import * as cores from "./cores.mjs";
import { execute } from "./host.mjs";

const TASKS = join(import.meta.dirname, "tasks");
const SPLITS = new Set(["development", "held-out"]);
const EFFECTS = new Set(["read", "write"]);
const KINDS = new Set(["Timeout", "Unavailable", "RateLimited", "NotFound", "PermissionDenied", "Invalid", "Conflict", "Failed"]);
const TASK_KINDS = new Set([
  "filter-group", "parallel", "conditional-call", "partial-failure", "retry", "polling", "pagination",
  "schema-detail", "core-composition", "string", "arithmetic", "ad-hoc-logic", "write",
]);

function validate(task, dir) {
  const problems = [];
  if (task.id !== dir) problems.push(`id ${task.id} does not match directory ${dir}`);
  if (!SPLITS.has(task.split)) problems.push(`split must be development or held-out`);
  if (typeof task["sx-expressible"] !== "boolean") problems.push(`sx-expressible must be a boolean`);
  if (typeof task.description !== "string" || !task.description) problems.push(`description is missing`);
  if (!["repomod", "other"].includes(task.domain)) problems.push(`domain must be repomod or other`);
  const prefix = task.split === "development" ? "dev-" : "held-";
  if (!dir.startsWith(prefix)) problems.push(`a ${task.split} task's id starts with ${prefix}`);
  for (const kind of task.kinds ?? []) {
    if (!TASK_KINDS.has(kind)) problems.push(`unknown kind ${kind}`);
  }
  const typeText = JSON.stringify([task.types ?? {}, task.enums ?? {}, task.capabilities ?? []]);
  const needsMissingType = /\b(Float|Bytes)\b/.test(typeText);
  const usesMap = /\bMap\b/.test(typeText);
  if (needsMissingType && task["sx-expressible"]) {
    problems.push(`sx-expressible must be false: Float or Bytes appears`);
  }
  if (!needsMissingType && !usesMap && !task["sx-expressible"]) {
    problems.push(`sx-expressible must be true: Map, Float, and Bytes do not appear`);
  }
  if (!("expected" in task)) problems.push(`expected is missing`);
  const declared = new Map();
  for (const capability of task.capabilities ?? []) {
    if (!EFFECTS.has(capability.effect)) problems.push(`${capability.id}: effect must be read or write`);
    if (capability.effect === "write" && typeof capability.idempotent !== "boolean") {
      problems.push(`${capability.id}: a write capability must declare idempotent`);
    }
    for (const field of ["id", "input", "output", "description"]) {
      if (typeof capability[field] !== "string") problems.push(`${capability.id}: ${field} is missing`);
    }
    declared.set(capability.id, capability);
  }
  for (const core of task.cores ?? []) {
    if (!(core in cores)) problems.push(`unknown core ${core}`);
  }
  for (const response of task.responses ?? []) {
    if (!declared.has(response.capability)) problems.push(`response for undeclared ${response.capability}`);
    if (!Array.isArray(response.results) || response.results.length === 0) {
      problems.push(`${response.capability}: results must be a non-empty list`);
      continue;
    }
    for (const result of response.results) {
      const keys = Object.keys(result);
      if (keys.length !== 1 || !(keys[0] === "output" || keys[0] === "error")) {
        problems.push(`${response.capability}: each result is {"output": ...} or {"error": ...}`);
      } else if (keys[0] === "error" && !KINDS.has(result.error.kind)) {
        problems.push(`${response.capability}: unknown error kind ${result.error.kind}`);
      }
    }
  }
  return problems;
}

async function check(dir) {
  const path = join(TASKS, dir);
  const task = JSON.parse(readFileSync(join(path, "task.json"), "utf8"));
  const problems = validate(task, dir);
  const reference = join(path, "reference.mjs");
  if (!existsSync(reference)) problems.push("reference.mjs is missing");
  if (problems.length) return problems;
  const solve = (await import(pathToFileURL(reference))).default;
  const run = await execute(task, solve);
  if (run.outcome !== "completed") return [`reference ${run.outcome}: ${run.error ?? run.reason}`];
  if (!isDeepStrictEqual(run.result, task.expected)) {
    return [`reference returned ${JSON.stringify(run.result)}, expected ${JSON.stringify(task.expected)}`];
  }
  return [];
}

const dirs = readdirSync(TASKS).filter((d) => existsSync(join(TASKS, d, "task.json"))).sort();
let failed = 0;
const counts = { development: 0, "held-out": 0 };
for (const dir of dirs) {
  const problems = await check(dir);
  const split = JSON.parse(readFileSync(join(TASKS, dir, "task.json"), "utf8")).split;
  counts[split] = (counts[split] ?? 0) + 1;
  if (problems.length) {
    failed += 1;
    console.log(`FAIL ${dir}`);
    for (const problem of problems) console.log(`  ${problem}`);
  } else {
    console.log(`ok   ${dir}`);
  }
}
console.log(`${dirs.length} tasks: ${counts.development} development, ${counts["held-out"]} held-out, ${failed} failing`);
process.exit(failed ? 1 : 0);
