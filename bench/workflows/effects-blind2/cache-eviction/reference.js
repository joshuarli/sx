const READ = tools.mcp__tools__fs_read_text;
const DELETE = tools.mcp__tools__fs_delete;
const ENTRIES = tools.mcp__tools__cachex_entries;

const read = async (path) => (await READ({ path })).content[0].text;
const budget = JSON.parse(await read("{{root}}/config/limits.json")).cache_bytes;
const pins = new Set((await read("{{root}}/config/pins.txt")).split("\n").map((l) => l.trim()).filter((l) => l !== "" && !l.startsWith("#")));
const entries = [];
let cursor;
do {
  let page;
  for (let attempt = 0; ; attempt++) {
    const answer = await ENTRIES(cursor === undefined ? {} : { cursor });
    if (!answer.isError) {
      page = answer.structuredContent;
      break;
    }
    if (attempt === 3) throw new Error("the index kept failing");
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  entries.push(...page.items);
  cursor = page.next ?? undefined;
} while (cursor !== undefined);
let remaining = entries.reduce((sum, e) => sum + e.bytes, 0);
const order = entries
  .filter((e) => !pins.has(e.key))
  .sort((a, b) => a.hits - b.hits || b.bytes - a.bytes || (a.key < b.key ? -1 : a.key > b.key ? 1 : 0));
const evicted = [];
for (const entry of order) {
  if (remaining <= budget) break;
  await DELETE({ path: `{{root}}/cache/${entry.path}` });
  remaining -= entry.bytes;
  evicted.push(entry.key);
}
return evicted;
