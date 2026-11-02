import { CallError, HostError, buildApi, type CallRecord, type Source } from "./api.ts";
import { McpClient, STANDARD_TIMEOUT_MS } from "./mcp.ts";

interface Options {
  solution: string;
  timeoutMs: number;
  command: string[];
  imported: { command: string[]; names: { [tool: string]: string } } | null;
}

function parseArguments(argv: string[]): Options {
  const rest = argv.slice(2);
  const separator = rest.indexOf("--");
  const own = separator < 0 ? rest : rest.slice(0, separator);
  const command = separator < 0 ? [] : rest.slice(separator + 1);
  let solution: string | undefined;
  let timeoutMs = STANDARD_TIMEOUT_MS;
  let importedCommand: string[] | undefined;
  let importedNames: { [tool: string]: string } | undefined;
  for (let at = 0; at < own.length; at += 2) {
    const value = own[at + 1];
    if (own[at] === "--solution" && value !== undefined) solution = value;
    else if (own[at] === "--timeout-ms" && value !== undefined) timeoutMs = Number(value);
    else if (own[at] === "--imported-command" && value !== undefined) importedCommand = JSON.parse(value) as string[];
    else if (own[at] === "--imported-names" && value !== undefined) importedNames = JSON.parse(value) as { [tool: string]: string };
    else throw new Error(`unrecognized argument ${own[at] ?? ""}`);
  }
  if (solution === undefined) throw new Error("--solution FILE is required");
  if (command.length === 0 && importedCommand === undefined) throw new Error("a server command is needed after --");
  if ((importedCommand === undefined) !== (importedNames === undefined)) {
    throw new Error("--imported-command and --imported-names go together");
  }
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) throw new Error("--timeout-ms must be a positive number");
  const imported = importedCommand === undefined || importedNames === undefined ? null : { command: importedCommand, names: importedNames };
  return { solution, timeoutMs, command, imported };
}

interface Report {
  outcome: "completed" | "failed";
  result?: unknown;
  kind?: "load" | "host" | "call" | "exception";
  error?: string;
  calls: CallRecord[];
  startupMs: number;
  elapsedMs: number;
}

async function main(): Promise<Report> {
  const options = parseArguments(process.argv);
  const began = performance.now();
  const clients: McpClient[] = [];
  try {
    const sources: Source[] = [];
    if (options.command.length > 0) {
      const client = await McpClient.connect(options.command, options.timeoutMs);
      clients.push(client);
      sources.push({ client, tools: await client.listTools(options.timeoutMs), names: null });
    }
    if (options.imported !== null) {
      const client = await McpClient.connect(options.imported.command, options.timeoutMs);
      clients.push(client);
      sources.push({ client, tools: await client.listTools(options.timeoutMs), names: options.imported.names });
    }
    const { api, calls } = buildApi(sources, options.timeoutMs);
    const startupMs = performance.now() - began;
    let report: Report;
    try {
      const loaded = (await import(options.solution)) as { default?: unknown };
      if (typeof loaded.default !== "function") {
        report = { outcome: "failed", kind: "load", error: "The module has no default export function.", calls, startupMs, elapsedMs: 0 };
      } else {
        const solve = loaded.default as (api: unknown) => Promise<unknown>;
        const started = performance.now();
        try {
          const result = await solve(api);
          report = { outcome: "completed", result: result === undefined ? null : result, calls, startupMs, elapsedMs: performance.now() - started };
        } catch (error) {
          const kind = error instanceof HostError ? "host" : error instanceof CallError ? "call" : "exception";
          const shown = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
          report = { outcome: "failed", kind, error: shown, calls, startupMs, elapsedMs: performance.now() - started };
        }
      }
    } catch (error) {
      const shown = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
      report = { outcome: "failed", kind: "load", error: shown, calls, startupMs, elapsedMs: 0 };
    }
    return report;
  } finally {
    await Promise.all(clients.map((client) => client.close()));
  }
}

try {
  process.stdout.write(JSON.stringify(await main()));
} catch (error) {
  const shown = error instanceof Error ? `${error.name}: ${error.message}` : String(error);
  process.stdout.write(JSON.stringify({ outcome: "failed", kind: "host", error: shown, calls: [], startupMs: 0, elapsedMs: 0 }));
}
