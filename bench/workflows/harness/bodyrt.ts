import { CallError } from "./api.ts";

export const logs: string[] = [];

type Options = { [option: string]: unknown };
type Step = { kind: "next"; state: unknown } | { kind: "done"; value: unknown };

const TRANSIENT = ["Timeout", "Unavailable", "RateLimited"];

const pause = (milliseconds: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, milliseconds));

const shown = (value: unknown): string => (typeof value === "string" ? value : (JSON.stringify(value) ?? "undefined"));

function backoffDelays(backoff: unknown, withinMs: number): number[] {
  const [first, cap] = backoff as [number, number];
  const delays: number[] = [];
  let total = 0;
  for (let delay = first; total + delay <= withinMs; delay = Math.min(delay * 2, cap)) {
    delays.push(delay);
    total += delay;
  }
  return delays;
}

async function retry<T>(options: Options, body: () => Promise<T>): Promise<T> {
  const kinds = (options.on as string[] | undefined) ?? TRANSIENT;
  const delays =
    options.delaysMs !== undefined
      ? (options.delaysMs as number[])
      : options.backoffMs !== undefined
        ? backoffDelays(options.backoffMs, options.withinMs as number)
        : [];
  for (let attempt = 0; ; attempt++) {
    try {
      return await body();
    } catch (error) {
      const retried = error instanceof CallError && kinds.includes(error.kind) && attempt < delays.length;
      if (!retried) throw error;
      await pause(delays[attempt] ?? 0);
    }
  }
}

async function waitUntil<T>(options: Options, poll: () => Promise<T>, done: (value: T) => boolean): Promise<T> {
  const withinMs = options.withinMs as number;
  const fixed = options.everyMs as number | undefined;
  const delays = fixed !== undefined ? null : backoffDelays(options.backoffMs, withinMs);
  let waited = 0;
  for (let round = 0; ; round++) {
    const value = await poll();
    if (done(value)) return value;
    const delay = fixed ?? delays?.[round] ?? Infinity;
    if (waited + delay > withinMs) throw new CallError("Timeout", `the condition did not hold within ${withinMs} ms`);
    waited += delay;
    await pause(delay);
  }
}

async function parMap<T, U>(options: Options, items: T[], body: (item: T) => Promise<U>): Promise<U[]> {
  const limit = options.limit as number;
  const results: U[] = new Array(items.length);
  let next = 0;
  const worker = async (): Promise<void> => {
    while (next < items.length) {
      const at = next++;
      results[at] = await body(items[at] as T);
    }
  };
  await Promise.all(Array.from({ length: Math.min(limit, items.length) }, worker));
  return results;
}

async function loop(options: Options, state: unknown, body: (state: unknown) => Step): Promise<unknown> {
  let current = state;
  for (let round = 0; round < (options.max as number); round++) {
    const step = body(current);
    if (step.kind === "done") return step.value;
    current = step.state;
  }
  throw new Error(`loop did not finish within ${String(options.max)} rounds`);
}

export function runtime(api: { [namespace: string]: unknown }) {
  const { sleep, ...tools } = api;
  return {
    tools,
    sleep,
    retry,
    waitUntil,
    parMap,
    loop,
    next: (state: unknown): Step => ({ kind: "next", state }),
    done: (value: unknown): Step => ({ kind: "done", value }),
    console: { log: (...values: unknown[]): void => void logs.push(values.map(shown).join(" ")) },
  };
}
