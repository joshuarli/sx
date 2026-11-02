export default async function main(api) {
  const levels = await api.stock.levels();
  const low = levels.filter((l) => l["on-hand"] - l.reserved < l["reorder-point"]);
  const plan = [];
  for (const level of low) {
    const quantity = level["reorder-qty"];
    const quote = await api.suppliers.quote({ sku: level.sku, quantity });
    plan.push({ sku: level.sku, quantity, cost: quote["unit-price"] * quantity });
  }
  return plan.sort((a, b) => (a.sku < b.sku ? -1 : 1));
}
