const READ = tools.mcp__tools__fs_read_text;
const STOCK = tools.mcp__tools__wh_stock;
const LEVEL = tools.mcp__tools__wh_level;
const ADJUST = tools.mcp__tools__wh_adjust;

const file = (await READ({ path: "{{root}}/counts/cycle.csv" })).content[0].text;
const counted = new Map();
for (const line of file.split("\n").slice(1)) {
  const cells = line.split(",");
  if (cells.length < 2 || cells[0].trim() === "") continue;
  counted.set(cells[0].trim(), Number(cells[1].trim()));
}
const stock = (await STOCK({})).structuredContent.result;
const adjusted = [];
for (const sku of [...counted.keys()].sort()) {
  const row = stock.find((r) => r.sku === sku);
  if (!row || row.frozen) continue;
  const delta = counted.get(sku) - row.on_hand;
  if (delta === 0) continue;
  const answer = await ADJUST({ sku, delta });
  if (answer.isError) {
    const now = (await LEVEL({ sku })).structuredContent;
    const missing = counted.get(sku) - now.on_hand;
    if (missing !== 0) await ADJUST({ sku, delta: missing });
  }
  adjusted.push({ sku, delta });
}
return adjusted;
