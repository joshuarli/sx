export default async function main(api: Api) {
  const order: [string, number][] = [["E-500", 2], ["A-100", 3], ["D-404", 2], ["C-300", 5], ["B-200", 1]];
  const looked = await Promise.all(
    order.map(async ([sku, quantity]) => {
      try {
        const item = await api.mcpInventory.lookupItem({ sku });
        return { sku, quantity, item };
      } catch {
        return { sku, quantity, item: null };
      }
    }),
  );
  looked.sort((a, b) => (a.sku < b.sku ? -1 : a.sku > b.sku ? 1 : 0));
  const reserved: { sku: string; reservation: string; cents: number }[] = [];
  const rejected: { sku: string; reason: string }[] = [];
  for (const { sku, quantity, item } of looked) {
    if (item === null) {
      rejected.push({ sku, reason: "unknown" });
    } else if (item.stock < quantity) {
      rejected.push({ sku, reason: "insufficient" });
    } else {
      const priority = item.tags.includes("perishable") ? ("rush" as const) : null;
      const made = await api.mcpInventory.reserveStock({ sku, quantity, priority });
      reserved.push({ sku, reservation: made.reservation, cents: item.cents * quantity });
    }
  }
  return { reserved, rejected, total: reserved.reduce((sum, line) => sum + line.cents, 0) };
}
