import { McpClient, RpcClosed, RpcError, RpcTimeout, type Tool, type ToolResult } from "./mcp.ts";

export const CALL_LIMIT = 1000;
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

const RESULT_META = "sx/result";
const WRAPPED = "wrapped";
const WRAPPED_KEY = "result";

type Kind = (typeof KINDS)[number];

export class CallError extends Error {
  kind: Kind;

  constructor(kind: Kind, message: string) {
    super(message);
    this.name = "CallError";
    this.kind = kind;
  }
}

export class HostError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "HostError";
  }
}

export interface CallRecord {
  capability: string;
  input: unknown;
  milliseconds: number;
  outcome: "ok" | Kind;
}

export const camel = (name: string): string =>
  name.replace(/-([a-z0-9])/g, (_, c: string) => c.toUpperCase()).replace(/\?$/, "");

const isKind = (value: unknown): value is Kind => KINDS.includes(value as Kind);

function texts(result: ToolResult): string {
  return result.content
    .filter((block) => block.type === "text")
    .map((block) => block.text ?? "")
    .join("");
}

function unwrap(tool: Tool, result: ToolResult, foreign: boolean): unknown {
  if (result.isError === true) {
    if (foreign) throw new CallError("Failed", texts(result));
    const kind = result.structuredContent?.kind;
    const message = result.structuredContent?.message;
    if (isKind(kind) && typeof message === "string") throw new CallError(kind, message);
    throw new HostError(`${tool.name} reported an error without a call error kind: ${texts(result)}`);
  }
  if (tool.outputSchema === undefined) return texts(result);
  const structured = result.structuredContent;
  if (structured === undefined) throw new HostError(`${tool.name} returned no structuredContent for its outputSchema`);
  return tool._meta?.[RESULT_META] === WRAPPED ? structured[WRAPPED_KEY] : structured;
}

function withoutNulls(payload: unknown): unknown {
  if (typeof payload !== "object" || payload === null || Array.isArray(payload)) return payload;
  return Object.fromEntries(Object.entries(payload).filter(([, value]) => value !== null));
}

export interface Built {
  api: { [namespace: string]: unknown };
  calls: CallRecord[];
}

export interface Source {
  client: McpClient;
  tools: Tool[];
  names: { [tool: string]: string } | null;
}

export function buildApi(sources: Source[], timeoutMs: number): Built {
  const calls: CallRecord[] = [];
  const api: { [namespace: string]: unknown } = {
    sleep: async (milliseconds: number): Promise<void> => {
      if (typeof milliseconds !== "number" || !Number.isFinite(milliseconds) || milliseconds < 0) {
        throw new HostError(`sleep takes milliseconds, got ${String(milliseconds)}`);
      }
      await new Promise((resolve) => setTimeout(resolve, milliseconds));
    },
  };
  const entries = sources.flatMap(({ client, tools, names }) => tools.map((tool) => ({ client, tool, names })));
  for (const { client, tool, names } of entries) {
    const capability = names === null ? tool.name : names[tool.name];
    if (capability === undefined) throw new HostError(`${tool.name} has no capability name`);
    const [namespace, name] = capability.split(".", 2);
    if (namespace === undefined || name === undefined) throw new HostError(`${capability} is not NAMESPACE.NAME`);
    const space = (api[camel(namespace)] ??= {}) as { [member: string]: unknown };
    space[camel(name)] = async (input?: unknown): Promise<unknown> => {
      if (calls.length >= CALL_LIMIT) throw new HostError(`more than ${CALL_LIMIT} tool calls`);
      const payload = input === undefined ? {} : structuredClone(input);
      const record: CallRecord = { capability, input: payload, milliseconds: 0, outcome: "ok" };
      calls.push(record);
      const started = performance.now();
      try {
        return unwrap(tool, await client.callTool(tool.name, names === null ? payload : withoutNulls(payload), timeoutMs), names !== null);
      } catch (error) {
        if (error instanceof CallError) record.outcome = error.kind;
        if (error instanceof RpcError) throw new HostError(`${capability} was rejected: ${error.message}`);
        if (error instanceof RpcTimeout) throw new CallError("Timeout", error.message);
        if (error instanceof RpcClosed) throw new HostError(`${capability}: ${error.message}`);
        throw error;
      } finally {
        record.milliseconds = performance.now() - started;
      }
    };
  }
  return { api, calls };
}
