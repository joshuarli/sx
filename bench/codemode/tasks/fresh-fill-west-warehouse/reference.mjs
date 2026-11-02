export default async function main(api) {
  const west = await api.stock.list({ warehouse: "west" });
  const east = await api.stock.list({ warehouse: "east" });
  const eastQuantity = new Map(east.map((r) => [r.sku, r.quantity]));
  const moved = [];
  const short = [];
  for (const row of west.filter((r) => r.quantity < 10).sort((a, b) => (a.sku < b.sku ? -1 : 1))) {
    const quantity = 10 - row.quantity;
    if ((eastQuantity.get(row.sku) ?? 0) < quantity) {
      short.push(row.sku);
      continue;
    }
    await api.transfers.create({ request: "fill-" + row.sku, sku: row.sku, from: "east", to: "west", quantity });
    moved.push({ sku: row.sku, quantity });
  }
  return { moved, short };
}
