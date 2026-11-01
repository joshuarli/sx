export default async function main(api) {
  const orders = await api.orders.list({ status: "open" });
  const totals = new Map();
  for (const order of orders) {
    const total = order.lines.reduce((sum, line) => sum + line.quantity * line.price, 0);
    totals.set(order.customer, (totals.get(order.customer) ?? 0) + total);
  }
  return [...totals]
    .map(([customer, total]) => ({ customer, total }))
    .sort((a, b) => (a.customer < b.customer ? -1 : 1));
}
