export default async function main(api) {
  const north = await api.warehouse.levels({ warehouse: "north" });
  const south = await api.warehouse.levels({ warehouse: "south" });
  const totals = new Map();
  for (const levels of [north, south]) {
    for (const [sku, count] of Object.entries(levels)) totals.set(sku, (totals.get(sku) ?? 0) + count);
  }
  return [...totals]
    .map(([sku, total]) => ({ sku, total }))
    .filter((row) => row.total < 10)
    .sort((a, b) => a.total - b.total || (a.sku < b.sku ? -1 : 1));
}
