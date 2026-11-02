const STOCK = tools.mcp__tools__warehouse_stock;
const ORDERS = tools.mcp__tools__warehouse_orders;
const ORDER = tools.mcp__tools__warehouse_order;

const openOrders = async (sku) => (await ORDERS({ sku })).structuredContent.result.filter((o) => o.status === "open");
const stock = (await STOCK({})).structuredContent.result;
const ordered = [];
for (const item of stock) {
  if (item.on_hand >= item.reorder_below) continue;
  if ((await openOrders(item.sku)).length > 0) continue;
  const request = { sku: item.sku, quantity: item.target - item.on_hand };
  if ((await ORDER(request)).isError && (await openOrders(item.sku)).length === 0) {
    const again = await ORDER(request);
    if (again.isError) throw new Error(`ordering ${item.sku} failed`);
  }
  ordered.push(item.sku);
}
return ordered;
