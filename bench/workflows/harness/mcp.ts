export const PROTOCOL_VERSION = "2025-06-18";
export const STANDARD_TIMEOUT_MS = 60_000;

const SHUTDOWN_GRACE_MS = 2_000;
const PAGE_LIMIT = 10_000;
const METHOD_NOT_FOUND = -32601;

type Json = null | boolean | number | string | Json[] | { [key: string]: Json };

export interface Tool {
  name: string;
  description?: string;
  inputSchema: Json;
  outputSchema?: Json;
  annotations?: { [key: string]: Json };
  _meta?: { [key: string]: Json };
}

export interface ToolResult {
  content: { type: string; text?: string }[];
  structuredContent?: { [key: string]: Json };
  isError?: boolean;
}

export class RpcError extends Error {
  code: number;

  constructor(code: number, message: string) {
    super(message);
    this.name = "RpcError";
    this.code = code;
  }
}

export class RpcTimeout extends Error {
  constructor(method: string, timeoutMs: number) {
    super(`${method} was not answered within ${timeoutMs} ms`);
    this.name = "RpcTimeout";
  }
}

export class RpcClosed extends Error {
  constructor(reason: string) {
    super(reason);
    this.name = "RpcClosed";
  }
}

interface Pending {
  resolve: (value: unknown) => void;
  reject: (error: Error) => void;
  timer: ReturnType<typeof setTimeout>;
}

interface Message {
  id?: number | string | null;
  method?: string;
  result?: unknown;
  error?: { code?: number; message?: string };
}

export class McpClient {
  private readonly child: BunSubprocess;
  private readonly pending = new Map<number, Pending>();
  private nextId = 0;
  private closed: string | null = null;
  private readonly reading: Promise<void>;

  private constructor(child: BunSubprocess) {
    this.child = child;
    this.reading = this.read();
  }

  static async connect(command: string[], timeoutMs: number = STANDARD_TIMEOUT_MS): Promise<McpClient> {
    if (command.length === 0) throw new RpcClosed("no server command");
    const child = Bun.spawn(command, { stdin: "pipe", stdout: "pipe", stderr: "inherit" });
    const client = new McpClient(child);
    try {
      const reply = await client.request(
        "initialize",
        { protocolVersion: PROTOCOL_VERSION, capabilities: {}, clientInfo: { name: "sx-workflows", version: "0" } },
        timeoutMs,
      );
      const version = (reply as { protocolVersion?: unknown } | null)?.protocolVersion;
      if (version !== PROTOCOL_VERSION) throw new RpcClosed(`the server speaks protocol version ${String(version)}`);
      client.notify("notifications/initialized", {});
    } catch (error) {
      await client.close();
      throw error;
    }
    return client;
  }

  private async read(): Promise<void> {
    const reader = this.child.stdout.getReader();
    const decoder = new TextDecoder();
    let buffered = "";
    try {
      for (;;) {
        const { done, value } = await reader.read();
        if (done) break;
        buffered += decoder.decode(value, { stream: true });
        let end = buffered.indexOf("\n");
        while (end >= 0) {
          const line = buffered.slice(0, end).trim();
          buffered = buffered.slice(end + 1);
          if (line !== "") this.dispatch(line);
          end = buffered.indexOf("\n");
        }
      }
      this.shutDown("the server closed its output or exited");
    } catch (error) {
      this.shutDown(`cannot read the server's output: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  private dispatch(line: string): void {
    let message: Message;
    try {
      message = JSON.parse(line) as Message;
    } catch {
      this.shutDown("the server wrote a line that is not JSON");
      return;
    }
    if (message.method !== undefined) {
      if (message.id !== undefined && message.id !== null) this.answerServerRequest(message.id, message.method);
      return;
    }
    if (typeof message.id !== "number") return;
    const waiting = this.pending.get(message.id);
    if (waiting === undefined) return;
    this.pending.delete(message.id);
    clearTimeout(waiting.timer);
    if (message.error !== undefined) {
      waiting.reject(new RpcError(message.error.code ?? 0, message.error.message ?? "no message"));
    } else {
      waiting.resolve(message.result);
    }
  }

  private answerServerRequest(id: number | string, method: string): void {
    if (method === "ping") {
      this.send({ jsonrpc: "2.0", id, result: {} });
    } else {
      this.send({ jsonrpc: "2.0", id, error: { code: METHOD_NOT_FOUND, message: `the client does not implement ${method}` } });
    }
  }

  private shutDown(reason: string): void {
    if (this.closed === null) this.closed = reason;
    for (const [id, waiting] of this.pending) {
      clearTimeout(waiting.timer);
      waiting.reject(new RpcClosed(reason));
      this.pending.delete(id);
    }
  }

  private send(message: { [key: string]: unknown }): void {
    if (this.closed !== null) return;
    try {
      this.child.stdin.write(`${JSON.stringify(message)}\n`);
      this.child.stdin.flush();
    } catch (error) {
      this.shutDown(`cannot write to the server: ${error instanceof Error ? error.message : String(error)}`);
    }
  }

  notify(method: string, params: unknown): void {
    this.send({ jsonrpc: "2.0", method, params });
  }

  request(method: string, params: unknown, timeoutMs: number = STANDARD_TIMEOUT_MS): Promise<unknown> {
    if (this.closed !== null) return Promise.reject(new RpcClosed(this.closed));
    this.nextId += 1;
    const id = this.nextId;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        this.notify("notifications/cancelled", { requestId: id, reason: "the client stopped waiting" });
        reject(new RpcTimeout(method, timeoutMs));
      }, timeoutMs);
      this.pending.set(id, { resolve, reject, timer });
      this.send({ jsonrpc: "2.0", id, method, params });
      if (this.closed !== null) this.shutDown(this.closed);
    });
  }

  async listTools(timeoutMs: number = STANDARD_TIMEOUT_MS): Promise<Tool[]> {
    const tools: Tool[] = [];
    const cursors = new Set<string>();
    let cursor: string | undefined;
    for (let page = 0; page < PAGE_LIMIT; page += 1) {
      const reply = (await this.request("tools/list", cursor === undefined ? {} : { cursor }, timeoutMs)) as {
        tools?: Tool[];
        nextCursor?: string | null;
      };
      if (!Array.isArray(reply.tools)) throw new RpcClosed("tools/list answered without a tools list");
      tools.push(...reply.tools);
      if (reply.nextCursor === undefined || reply.nextCursor === null) return tools;
      if (cursors.has(reply.nextCursor)) throw new RpcClosed(`tools/list repeated the cursor ${reply.nextCursor}`);
      cursors.add(reply.nextCursor);
      cursor = reply.nextCursor;
    }
    throw new RpcClosed(`tools/list went past ${PAGE_LIMIT} pages`);
  }

  async callTool(name: string, args: unknown, timeoutMs: number = STANDARD_TIMEOUT_MS): Promise<ToolResult> {
    return (await this.request("tools/call", { name, arguments: args }, timeoutMs)) as ToolResult;
  }

  async close(): Promise<void> {
    this.shutDown("the connection was closed");
    try {
      await this.child.stdin.end();
    } catch {
      this.child.kill();
    }
    let timer: ReturnType<typeof setTimeout> | undefined;
    const grace = new Promise<"late">((resolve) => {
      timer = setTimeout(() => resolve("late"), SHUTDOWN_GRACE_MS);
    });
    const outcome = await Promise.race([this.child.exited, grace]);
    clearTimeout(timer);
    if (outcome === "late") this.child.kill();
    await this.child.exited;
    await this.reading;
  }
}
