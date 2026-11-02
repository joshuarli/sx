export const KINDS = [
  "Timeout",
  "Unavailable",
  "RateLimited",
  "NotFound",
  "PermissionDenied",
  "Invalid",
  "Conflict",
  "Failed",
] as const;
export type Kind = (typeof KINDS)[number];
export const TRANSIENT: readonly Kind[] = ["Timeout", "Unavailable", "RateLimited"];
export const SAFE_INTEGER = 2 ** 53;

export class CallError extends Error {
  declare kind: Kind;

  constructor(kind: Kind, message: string) {
    super();
    this.kind = kind;
    this.message = message;
  }
}
Object.defineProperty(CallError.prototype, "name", { value: "CallError" });

export type TrapClass = "arithmetic" | "out-of-range" | "loop-limit";

export class Trap extends Error {
  constructor(
    readonly trap: TrapClass,
    message: string,
  ) {
    super(message);
  }
}

export class UnexpectedCall extends Error {
  constructor(
    readonly capability: string,
    readonly input: unknown,
    readonly reason: "no-fixture" | "repeated-write",
  ) {
    super(`${capability} has no fixture for this input: ${JSON.stringify(input)} (${reason})`);
  }
}

export const isUncatchable = (error: unknown): boolean => error instanceof Trap || error instanceof UnexpectedCall;

export type Json = null | boolean | number | string | Json[] | { [key: string]: Json };
export type FixtureResult = { output: Json } | { error: { kind: Kind; message: string } };
export interface Entry {
  capability: string;
  input: Json;
  results: FixtureResult[];
  served?: number;
}
export interface ToolSpec {
  id: string;
  effect: "read" | "write";
}
export interface CallRecord {
  capability: string;
  input: Json;
}

export function sameJson(a: unknown, b: unknown): boolean {
  if (typeof a === "number" && typeof b === "number") return a === b;
  if (typeof a !== typeof b || a === null || b === null || typeof a !== "object") return a === b;
  if (Array.isArray(a) || Array.isArray(b)) {
    return Array.isArray(a) && Array.isArray(b) && a.length === b.length && a.every((item, i) => sameJson(item, b[i]));
  }
  const left = Object.keys(a as object).sort();
  const right = Object.keys(b as object).sort();
  return (
    left.length === right.length &&
    left.every((key, i) => key === right[i] && sameJson((a as Record<string, unknown>)[key], (b as Record<string, unknown>)[key]))
  );
}

export class FixtureHost {
  readonly calls: CallRecord[] = [];
  private readonly entries: Required<Entry>[];

  constructor(
    entries: Entry[],
    private readonly effects: Map<string, "read" | "write">,
  ) {
    this.entries = entries.map((entry) => ({ ...entry, input: withoutNulls(entry.input), served: 0 }));
  }

  record(capability: string, input: Json): void {
    this.calls.push({ capability, input });
  }

  answer(capability: string, input: Json): Json {
    const entry = this.entries.find((e) => e.capability === capability && sameJson(e.input, input));
    if (entry === undefined || entry.results.length === 0) throw new UnexpectedCall(capability, input, "no-fixture");
    if (this.effects.get(capability) === "write" && entry.served >= entry.results.length) {
      throw new UnexpectedCall(capability, input, "repeated-write");
    }
    const result = entry.results[Math.min(entry.served, entry.results.length - 1)]!;
    entry.served += 1;
    if ("error" in result) throw new CallError(result.error.kind, result.error.message);
    return structuredClone(result.output);
  }
}

export const camel = (name: string): string =>
  name.replace(/-([a-z0-9])/g, (_, c: string) => c.toUpperCase()).replace(/\?$/, "");

function withoutNulls(input: unknown): Json {
  if (input === undefined) return {};
  if (typeof input !== "object" || input === null || Array.isArray(input)) return input as Json;
  return Object.fromEntries(Object.entries(input).filter(([, value]) => value !== null)) as Json;
}

export function checkedNumber(value: number, where: string): void {
  if (!Number.isFinite(value)) throw new Trap("arithmetic", `${where}: ${String(value)}`);
  if (Number.isInteger(value) && Math.abs(value) > SAFE_INTEGER) throw new Trap("arithmetic", `${where}: ${value} is not a safe integer`);
}

export function checkedJson(value: unknown, where = "$"): Json {
  switch (typeof value) {
    case "undefined":
      throw new Trap("out-of-range", `${where}: undefined`);
    case "number":
      checkedNumber(value, where);
      return value;
    case "string":
    case "boolean":
      return value;
    case "object": {
      if (value === null) return null;
      if (Array.isArray(value)) return Array.from(value, (item, i) => checkedJson(item, `${where}[${i}]`));
      const out: { [key: string]: Json } = {};
      for (const key of Object.keys(value)) out[key] = checkedJson((value as Record<string, unknown>)[key], `${where}.${key}`);
      return out;
    }
    default:
      throw new TypeError(`${where}: a ${typeof value} has no JSON encoding`);
  }
}

export function formatLog(args: unknown[]): string {
  return args
    .map((arg, i) => {
      if (typeof arg === "string") return arg;
      if (arg === null) return "null";
      const json = checkedJson(arg, `console.log argument ${i}`);
      return typeof json === "object" ? JSON.stringify(json) : String(json);
    })
    .join(" ");
}

export class Step {
  constructor(
    readonly tag: "next" | "done",
    readonly payload: unknown,
  ) {}
}

type Rejected = { status: "rejected"; reason: unknown };
type Fulfilled = { status: "fulfilled"; value: unknown };

function settled(result: Fulfilled | Rejected): unknown {
  if (result.status === "fulfilled") return { status: "fulfilled", value: result.value };
  const shape = { status: "rejected", reason: result.reason };
  return new Proxy(shape, {
    get(target, property, receiver) {
      if (property === "value") throw new Trap("out-of-range", "value of a rejected Settled");
      return Reflect.get(target, property, receiver);
    },
  });
}

interface RetryOptions {
  delaysMs?: number[];
  backoffMs?: [number, number];
  withinMs?: number;
  on?: Kind[];
}
interface WaitOptions {
  everyMs?: number;
  backoffMs?: [number, number];
  withinMs: number;
}

const kindOf = (error: unknown): Kind | null => (error instanceof CallError ? error.kind : null);

export class Environment {
  readonly logs: string[] = [];
  readonly delays: number[] = [];
  readonly host: FixtureHost;
  readonly tools: { [namespace: string]: { [name: string]: (input?: unknown) => Promise<unknown> } } = {};

  constructor(specs: ToolSpec[], entries: Entry[]) {
    this.host = new FixtureHost(entries, new Map(specs.map((s) => [s.id, s.effect])));
    for (const { id } of specs) {
      const cut = id.indexOf(".");
      const namespace = camel(id.slice(0, cut));
      const name = camel(id.slice(cut + 1));
      (this.tools[namespace] ??= {})[name] = async (input?: unknown) => {
        const payload = withoutNulls(input);
        checkedJson(payload, `${id} input`);
        this.host.record(id, payload);
        await null;
        return this.host.answer(id, payload);
      };
    }
  }

  readonly console = { log: (...args: unknown[]): void => void this.logs.push(formatLog(args)) };

  readonly sleep = async (ms: number): Promise<void> => {
    if (ms > 0) this.delays.push(ms);
  };

  readonly next = (state: unknown): Step => new Step("next", state);
  readonly done = (value: unknown): Step => new Step("done", value);

  readonly retry = async <T>(options: RetryOptions, body: () => Promise<T>): Promise<T> => {
    const retried = options.on ?? TRANSIENT;
    let waited = 0;
    for (let attempt = 0; ; attempt++) {
      try {
        return await body();
      } catch (error) {
        const kind = isUncatchable(error) ? null : kindOf(error) ?? (error instanceof Error ? "Failed" : null);
        if (kind === null || !retried.includes(kind)) throw error;
        let delay: number | undefined;
        if (options.delaysMs !== undefined) {
          delay = options.delaysMs[attempt];
        } else if (options.backoffMs !== undefined) {
          const [first, cap] = options.backoffMs;
          const next = Math.min(first * 2 ** attempt, cap);
          if (waited + next <= (options.withinMs ?? Infinity)) delay = next;
        }
        if (delay === undefined) throw error;
        waited += delay;
        this.delays.push(delay);
      }
    }
  };

  readonly waitUntil = async <T>(options: WaitOptions, poll: () => Promise<T>, done: (value: T) => boolean): Promise<T> => {
    let waited = 0;
    for (let round = 0; ; round++) {
      const value = await poll();
      if (done(value)) return value;
      let interval = options.everyMs ?? 0;
      if (options.backoffMs !== undefined) interval = Math.min(options.backoffMs[0] * 2 ** round, options.backoffMs[1]);
      if (waited + interval > options.withinMs) throw new CallError("Timeout", `wait-until gave up after ${options.withinMs} ms.`);
      waited += interval;
      this.delays.push(interval);
    }
  };

  readonly parMap = async <T, U>(options: { limit: number }, items: T[], body: (item: T) => Promise<U>): Promise<U[]> => {
    const results: U[] = new Array(items.length);
    const failures: { error: unknown }[] = new Array(items.length);
    let cursor = 0;
    const worker = async (): Promise<void> => {
      for (;;) {
        const index = cursor++;
        if (index >= items.length) return;
        try {
          results[index] = await body(items[index] as T);
        } catch (error) {
          failures[index] = { error };
        }
      }
    };
    await Promise.all(Array.from({ length: Math.min(options.limit, items.length) }, worker));
    for (let i = 0; i < items.length; i++) if (failures[i] !== undefined) throw failures[i]!.error;
    return results;
  };

  readonly loop = async <S, R>(options: { max: number }, initial: S, body: (state: S) => Promise<Step> | Step): Promise<R> => {
    let state = initial;
    for (let evaluation = 1; evaluation <= options.max; evaluation++) {
      const step = await body(state);
      if (!(step instanceof Step)) throw new TypeError("a loop body must return next(...) or done(...)");
      if (step.tag === "done") return step.payload as R;
      if (evaluation === options.max) throw new Trap("loop-limit", `loop reached its max of ${options.max}`);
      state = step.payload as S;
    }
    throw new Trap("loop-limit", "loop with a max below 1");
  };

  readonly Promise = {
    all: async (items: unknown[]): Promise<unknown[]> => {
      const results = await Promise.allSettled(items);
      const failed = results.find((r) => r.status === "rejected");
      if (failed !== undefined) throw (failed as PromiseRejectedResult).reason;
      return results.map((r) => (r as PromiseFulfilledResult<unknown>).value);
    },
    allSettled: async (items: unknown[]): Promise<unknown[]> => {
      const results = await Promise.allSettled(items);
      for (const result of results) if (result.status === "rejected" && isUncatchable(result.reason)) throw result.reason;
      return results.map((r) => settled(r));
    },
  };

  readonly Math = {
    floor: (x: number): number => rounded(Math.floor(x)),
    ceil: (x: number): number => rounded(Math.ceil(x)),
    round: (x: number): number => rounded(Math.round(x)),
    trunc: (x: number): number => rounded(Math.trunc(x)),
    abs: Math.abs,
    max: Math.max,
    min: Math.min,
  };

  readonly parseInt = (text: string, radix: number): number => {
    const value = Number.parseInt(text, radix);
    if (Number.isNaN(value)) throw new Trap("arithmetic", `parseInt: no digits in ${JSON.stringify(text)}`);
    checkedNumber(value, "parseInt");
    return value;
  };
}

const withinInt = (x: unknown): boolean => typeof x === "number" && Number.isInteger(x) && Math.abs(x) <= SAFE_INTEGER;

function rounded(value: number): number {
  if (!Number.isFinite(value)) throw new Trap("arithmetic", `rounding gave ${String(value)}`);
  if (Math.abs(value) > SAFE_INTEGER) throw new Trap("out-of-range", `${value} is outside the Int range`);
  return value;
}

export function guards(): Record<string, (...args: never[]) => unknown> {
  const splits = (text: string, index: number): boolean => {
    const before = text.charCodeAt(index - 1);
    const at = text.charCodeAt(index);
    return before >= 0xd800 && before <= 0xdbff && at >= 0xdc00 && at <= 0xdfff;
  };
  return {
    a(op: string, left: unknown, right: unknown): unknown {
      let value: unknown;
      switch (op) {
        case "+":
          value = (left as number) + (right as number);
          break;
        case "-":
          value = (left as number) - (right as number);
          break;
        case "*":
          value = (left as number) * (right as number);
          break;
        case "/":
          value = (left as number) / (right as number);
          break;
        default:
          value = (left as number) % (right as number);
      }
      if (typeof value !== "number") return value;
      if (!Number.isFinite(value)) throw new Trap("arithmetic", `${String(left)} ${op} ${String(right)} is ${String(value)}`);
      if (withinInt(left) && withinInt(right) && op !== "/" && op !== "%") {
        const [x, y] = [BigInt(left as number), BigInt(right as number)];
        const exact = op === "+" ? x + y : op === "-" ? x - y : x * y;
        if (exact > BigInt(SAFE_INTEGER) || exact < -BigInt(SAFE_INTEGER)) {
          throw new Trap("arithmetic", `${String(left)} ${op} ${String(right)} is ${exact}, not a safe integer`);
        }
      }
      return value;
    },
    i(target: unknown, key: unknown): unknown {
      if (Array.isArray(target)) {
        if (typeof key !== "number" || !Number.isInteger(key) || key < 0 || key >= target.length) {
          throw new Trap("out-of-range", `index ${String(key)} of a list of ${target.length}`);
        }
        return target[key];
      }
      if (typeof target === "string" && typeof key === "number") {
        if (!Number.isInteger(key) || key < 0 || key >= target.length) throw new Trap("out-of-range", `index ${key} of a string of ${target.length}`);
        if (splits(target, key) || splits(target, key + 1)) throw new Trap("arithmetic", `index ${key} cuts a surrogate pair`);
        return target[key];
      }
      return (target as Record<string, unknown>)[key as string];
    },
    d(value: unknown, wanted: number): unknown {
      if (Array.isArray(value) && value.length < wanted) throw new Trap("out-of-range", `destructuring ${wanted} from a list of ${value.length}`);
      return value;
    },
    c(error: unknown): void {
      if (isUncatchable(error)) throw error;
    },
    sl(target: unknown, ...args: number[]): unknown {
      if (typeof target !== "string") return (target as unknown[]).slice(...args);
      const length = target.length;
      const clamp = (n: number | undefined, fallback: number): number => {
        if (n === undefined) return fallback;
        const integer = Math.trunc(n) || 0;
        return integer < 0 ? Math.max(length + integer, 0) : Math.min(integer, length);
      };
      const start = clamp(args[0], 0);
      const end = clamp(args[1], length);
      if (start < end && (splits(target, start) || splits(target, end))) throw new Trap("arithmetic", "slice cuts a surrogate pair");
      return target.slice(...args);
    },
  };
}
