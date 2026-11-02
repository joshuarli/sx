import { McpClient, RpcError, STANDARD_TIMEOUT_MS } from "./mcp.ts";

const SERVER = "tools";
const AsyncFunction = Object.getPrototypeOf(async () => undefined).constructor as new (
  ...parts: string[]
) => (...values: unknown[]) => Promise<unknown>;

class Exit extends Error {}

interface Options {
  script: string;
  command: string[];
}

function parseArguments(argv: string[]): Options {
  const rest = argv.slice(2);
  const separator = rest.indexOf("--");
  const own = separator < 0 ? rest : rest.slice(0, separator);
  const command = separator < 0 ? [] : rest.slice(separator + 1);
  if (own.length !== 2 || own[0] !== "--script" || own[1] === undefined) throw new Error("--script FILE is required");
  if (command.length === 0) throw new Error("a server command is needed after --");
  return { script: own[1], command };
}

const identifier = (name: string): string => `mcp__${SERVER}__${name.replace(/[^A-Za-z0-9_]/g, "_")}`;

const shown = (value: unknown): string => (typeof value === "string" ? value : (JSON.stringify(value) ?? "undefined"));

async function main(): Promise<unknown> {
  const options = parseArguments(process.argv);
  const client = await McpClient.connect(options.command);
  try {
    const tools: { [name: string]: (args?: unknown) => Promise<unknown> } = {};
    const calls: { tool: string; args: unknown }[] = [];
    for (const tool of await client.listTools()) {
      tools[identifier(tool.name)] = async (args?: unknown): Promise<unknown> => {
        calls.push({ tool: identifier(tool.name), args: args ?? {} });
        try {
          return await client.callTool(tool.name, args ?? {}, STANDARD_TIMEOUT_MS);
        } catch (error) {
          if (error instanceof RpcError) throw new Error(error.message);
          throw error;
        }
      };
    }
    const output: string[] = [];
    const text = (value: unknown): void => {
      output.push(shown(value));
    };
    const log = (...values: unknown[]): void => {
      output.push(values.map(shown).join(" "));
    };
    const console = { log, info: log, warn: log, error: log, debug: log };
    const exit = (): never => {
      throw new Exit();
    };
    const source = await Bun.file(options.script).text();
    const run = new AsyncFunction("tools", "text", "console", "exit", source);
    try {
      const returned = await run(tools, text, console, exit);
      if (returned !== undefined) text(returned);
      return { outcome: "completed", output, calls };
    } catch (error) {
      if (error instanceof Exit) return { outcome: "completed", output, calls };
      const message = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      return { outcome: "failed", output, calls, error: message };
    }
  } finally {
    await client.close();
  }
}

process.stdout.write(JSON.stringify(await main()));
