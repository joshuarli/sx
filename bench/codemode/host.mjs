import { isDeepStrictEqual } from "node:util";
import * as cores from "./cores.mjs";

const CALL_LIMIT = 1000;

export class CallError extends Error {
  constructor(kind, message) {
    super(message);
    this.name = "CallError";
    this.kind = kind;
  }
}

export class HostError extends Error {
  constructor(message) {
    super(message);
    this.name = "HostError";
  }
}

export const camel = (name) => name.replace(/-([a-z0-9])/g, (_, c) => c.toUpperCase()).replace(/\?$/, "");

export function makeHost(task) {
  const capabilities = new Map(task.capabilities.map((c) => [c.id, c]));
  const entries = task.responses.map((response) => ({ ...response, served: 0 }));
  const log = { calls: [], delays: [] };
  const api = {
    sleep: async (ms) => {
      if (typeof ms !== "number" || !Number.isFinite(ms) || ms < 0) throw new HostError(`sleep takes milliseconds, got ${ms}`);
      log.delays.push(ms);
    },
  };
  for (const core of task.cores ?? []) {
    api[core] = Object.fromEntries(Object.entries(cores[core]));
  }
  for (const id of capabilities.keys()) {
    const [namespace, name] = id.split(".");
    api[namespace] ??= {};
    api[namespace][camel(name)] = async (input) => {
      if (log.calls.length >= CALL_LIMIT) throw new HostError(`more than ${CALL_LIMIT} tool calls`);
      const capability = capabilities.get(id);
      const payload = input === undefined && capability.input === "Record {}" ? {} : structuredClone(input);
      log.calls.push({ capability: id, input: payload });
      const entry = entries.find((e) => e.capability === id && isDeepStrictEqual(e.input, payload));
      if (!entry) throw new HostError(`${id} was called with an input it does not accept: ${JSON.stringify(payload)}`);
      if (capability.effect === "write" && entry.served >= entry.results.length) {
        throw new HostError(`${id} ${JSON.stringify(payload)} ran more times than the task allows`);
      }
      const result = entry.results[Math.min(entry.served, entry.results.length - 1)];
      entry.served += 1;
      if ("error" in result) throw new CallError(result.error.kind, result.error.message);
      return structuredClone(result.output);
    };
  }
  const unperformedWrites = () =>
    entries
      .filter((e) => capabilities.get(e.capability).effect === "write" && e.served !== e.results.length)
      .map((e) => `${e.capability} ${JSON.stringify(e.input)} ran ${e.served} of ${e.results.length} times`);
  return { api, log, unperformedWrites };
}

export async function execute(task, solve) {
  const { api, log, unperformedWrites } = makeHost(task);
  try {
    const result = await solve(api);
    const missing = unperformedWrites();
    if (missing.length) return { outcome: "wrong", reason: `a required write did not happen: ${missing[0]}`, result, log };
    return { outcome: "completed", result: result === undefined ? null : result, log };
  } catch (error) {
    const kind = error instanceof HostError ? "host" : error instanceof CallError ? "call" : "exception";
    return { outcome: "failed", kind, error: `${error.name}: ${error.message}`, log };
  }
}
