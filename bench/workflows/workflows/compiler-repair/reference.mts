type Diagnostic = { line: number; column: number; message: string };

const MARKER = ": error: ";

function diagnostics(stderr: string): Diagnostic[] {
  const found: Diagnostic[] = [];
  for (const line of stderr.split("\n")) {
    const at = line.indexOf(MARKER);
    if (at < 0) continue;
    const where = line.slice(0, at).split(":");
    const lineNumber = Number(where[1]);
    const column = Number(where[2]);
    if (Number.isInteger(lineNumber) && Number.isInteger(column)) {
      found.push({ line: lineNumber, column, message: line.slice(at + MARKER.length) });
    }
  }
  return found;
}

export default async function main(api: Api) {
  const path = "{{root}}/src/main.c";
  const fixed: number[] = [];
  for (let attempts = 1; ; attempts++) {
    const run = await api.process.run({
      program: "/usr/bin/cc",
      args: ["-fsyntax-only", "main.c"],
      cwd: "{{root}}/src",
      stdin: null,
    });
    const found = diagnostics(run.stderr);
    const fixable = found.filter((d) => d.message.startsWith("expected ';'"));
    if (run.status === 0 || fixable.length === 0 || attempts === 6) {
      return { attempts, fixed, status: run.status, diagnostics: found };
    }
    const target = fixable[0]!.line;
    const lines = (await api.fs.readText({ path })).split("\n");
    lines[target - 1] += ";";
    await api.fs.writeText({ path, text: lines.join("\n") });
    fixed.push(target);
  }
}
