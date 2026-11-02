const ORDERS = tools.mcp__tools__shop_orders;
const MKDIR = tools.mcp__tools__fs_make_directory;
const WRITE = tools.mcp__tools__fs_write_text;

const pages = [];
let page = 1;
while (page !== null) {
  let answer;
  for (let tries = 0; tries < 3; tries++) {
    answer = await ORDERS({ page });
    if (!answer.isError) break;
  }
  if (answer.isError) return { exported: false, failed_page: page };
  pages.push({ number: page, items: answer.structuredContent.items });
  page = answer.structuredContent.next ?? null;
}

const dir = "{{root}}/exports/2026-10";
const write = async (path, text) => {
  const done = await WRITE({ path, text });
  if (done.isError) throw new Error(`write ${path} failed`);
};
await MKDIR({ path: dir });
let rows = 0;
for (const { number, items } of pages) {
  const lines = ["id,placed,customer,total_cents", ...items.map((o) => `${o.id},${o.placed},${o.customer},${o.total_cents}`)];
  await write(`${dir}/orders-0${number}.csv`, lines.join("\n") + "\n");
  rows += items.length;
}
await write(`${dir}/manifest.json`, `{"files": ${pages.length}, "rows": ${rows}}\n`);
return { exported: true, failed_page: 0 };
