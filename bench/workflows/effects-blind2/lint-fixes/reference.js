const READ = tools.mcp__tools__fs_read_text;
const WRITE = tools.mcp__tools__fs_write_text;

const read = async (path) => (await READ({ path })).content[0].text;
const report = await read("{{root}}/review/findings.txt");
const byPath = new Map();
for (const line of report.split("\n")) {
  const found = /^(\S+):(\d+): (.+) -> (.+)$/.exec(line);
  if (!found) continue;
  if (!byPath.has(found[1])) byPath.set(found[1], []);
  byPath.get(found[1]).push({ line: Number(found[2]), wrong: found[3], right: found[4] });
}
const ranked = [...byPath.entries()]
  .sort(([pa, a], [pb, b]) => b.length - a.length || (pa < pb ? -1 : pa > pb ? 1 : 0))
  .slice(0, 5);
for (const [path, own] of ranked) {
  const full = `{{root}}/${path}`;
  const lines = (await read(full)).split("\n");
  const fixed = lines.map((text, i) =>
    own.filter((f) => f.line === i + 1).reduce((acc, f) => acc.split(f.wrong).join(f.right), text),
  );
  await WRITE({ path: full, text: fixed.join("\n") });
}
return ranked.map(([path]) => path);
