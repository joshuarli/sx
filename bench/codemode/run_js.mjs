import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { pathToFileURL } from "node:url";
import { execute } from "./host.mjs";

const [taskPath, solutionPath] = process.argv.slice(2);
const task = JSON.parse(readFileSync(taskPath, "utf8"));
let solve;
try {
  solve = (await import(pathToFileURL(resolve(solutionPath)))).default;
} catch (error) {
  process.stdout.write(JSON.stringify({ outcome: "failed", kind: "load", error: `${error.name}: ${error.message}`, log: { calls: [], delays: [] } }));
  process.exit(0);
}
if (typeof solve !== "function") {
  process.stdout.write(JSON.stringify({ outcome: "failed", kind: "load", error: "The module has no default export function.", log: { calls: [], delays: [] } }));
  process.exit(0);
}
process.stdout.write(JSON.stringify(await execute(task, solve)));
