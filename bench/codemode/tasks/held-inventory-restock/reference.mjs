export default async function main(api) {
  const items = (await api.inventory.list()).sort((a, b) => (a.sku < b.sku ? -1 : 1));
  const out = [];
  for (const item of items) {
    const available = item["on-hand"] - item.reserved;
    if (available >= item.reorder) continue;
    const qty = 2 * item.reorder - available;
    const { po } = await api.purchasing.createPo({ sku: item.sku, qty });
    out.push({ sku: item.sku, qty, po });
  }
  return out;
}
