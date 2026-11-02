import { readFileSync } from "node:fs";
import { instrument } from "./instrument.ts";
import {
  CallError,
  Environment,
  Trap,
  UnexpectedCall,
  checkedJson,
  guards,
  type CallRecord,
  type Entry,
  type Json,
  type Kind,
  type ToolSpec,
  type TrapClass,
} from "./runtime.ts";

export type Failure =
  | { class: "call-failed"; kind: Kind; message: string }
  | { class: "thrown"; message: string }
  | { class: "trap"; trap: TrapClass; message: string }
  | { class: "unexpected-call"; capability: string; input: Json; message: string }
  | { class: "js-error"; name: string; message: string };

export interface RunOutput {
  result: Json | null;
  logs: string[];
  calls: CallRecord[];
  delays: number[];
  error?: Failure;
}

const AsyncFunction = Object.getPrototypeOf(async () => {}).constructor as new (...args: string[]) => (...values: unknown[]) => Promise<unknown>;
const transpiler = new Bun.Transpiler({ loader: "ts" });

export function lower(source: string): string {
  const wrapped = transpiler.transformSync(`async function __program() {\n${source}\n}`);
  const open = wrapped.indexOf("{");
  const close = wrapped.lastIndexOf("}");
  return instrument(wrapped.slice(open + 1, close));
}

export function failureOf(error: unknown): Failure {
  if (error instanceof Trap) return { class: "trap", trap: error.trap, message: error.message };
  if (error instanceof UnexpectedCall) {
    return { class: "unexpected-call", capability: error.capability, input: error.input as Json, message: error.message };
  }
  if (error instanceof CallError) return { class: "call-failed", kind: error.kind, message: error.message };
  if (error instanceof Error && error.constructor === Error) return { class: "thrown", message: error.message };
  const named = error instanceof Error ? error : new Error(String(error));
  return { class: "js-error", name: named.name, message: named.message };
}

export async function runProgram(source: string, specs: ToolSpec[], entries: Entry[]): Promise<RunOutput> {
  const environment = new Environment(specs, entries);
  const output = (extra: Partial<RunOutput>): RunOutput => ({
    result: null,
    logs: environment.logs,
    calls: environment.host.calls,
    delays: environment.delays,
    ...extra,
  });
  try {
    const body = lower(source);
    const names = ["tools", "console", "sleep", "retry", "waitUntil", "parMap", "loop", "next", "done", "Promise", "Math", "parseInt", "$"];
    const values = [
      environment.tools,
      environment.console,
      environment.sleep,
      environment.retry,
      environment.waitUntil,
      environment.parMap,
      environment.loop,
      environment.next,
      environment.done,
      environment.Promise,
      environment.Math,
      environment.parseInt,
      guards(),
    ];
    const run = new AsyncFunction(...names, `"use strict";\nreturn (async () => {\n${body}\n})();`);
    const value = await run(...values);
    return output({ result: checkedJson(value === undefined ? null : value) });
  } catch (error) {
    return output({ error: failureOf(error) });
  }
}

if (import.meta.main) {
  const args = process.argv.slice(2);
  const flag = (name: string): string => {
    const at = args.indexOf(name);
    if (at < 0 || at + 1 >= args.length) throw new Error(`shim: ${name} is required`);
    return args[at + 1]!;
  };
  const program = args.find((a, i) => !a.startsWith("--") && (i === 0 || !args[i - 1]!.startsWith("--")));
  if (program === undefined) throw new Error("shim: a program file is required");
  const specs = JSON.parse(readFileSync(flag("--tools"), "utf8")) as ToolSpec[];
  const entries = JSON.parse(readFileSync(flag("--responses"), "utf8")) as Entry[];
  const outcome = await runProgram(readFileSync(program, "utf8"), specs, entries);
  process.stdout.write(`${JSON.stringify(outcome)}\n`);
}
