const READ = tools.mcp__tools__fs_read_text;
const WRITE = tools.mcp__tools__fs_write_text;
const RATE = tools.mcp__tools__carrier_rate;

const raw = (await READ({ path: "{{root}}/shipping/shipments.csv" })).content[0].text;
const rows = raw.split("\n").slice(1).filter((line) => line.trim() !== "").map((line) => line.split(","));
const prices = new Map();
for (const [, zip, kilograms] of rows) {
  const key = `${zip}/${kilograms}`;
  if (prices.has(key)) continue;
  let answer;
  for (let tries = 0; tries < 2; tries++) {
    answer = await RATE({ to_zip: zip, weight_kg: Number(kilograms) });
    if (!answer.isError) break;
  }
  if (answer.isError) throw new Error(`no price for ${key}`);
  prices.set(key, answer.structuredContent.price_cents);
}
const lines = rows.map(([id, zip, kilograms]) => `${id},${prices.get(`${zip}/${kilograms}`)}`);
const written = await WRITE({ path: "{{root}}/shipping/quotes.csv", text: ["id,price_cents", ...lines].join("\n") + "\n" });
if (written.isError) throw new Error("quotes.csv was not written");
return rows.length;
