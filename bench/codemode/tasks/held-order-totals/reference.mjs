export default async function main(api) {
  const orders = await api.orders.list();
  const totals = orders.map((order) => {
    const subtotal = order.lines.reduce((sum, l) => sum + l.qty * l.cents, 0);
    const discount = Math.floor((subtotal * order.discount) / 100);
    return { id: order.id, total: subtotal - discount };
  });
  totals.sort((a, b) => b.total - a.total || (a.id < b.id ? -1 : 1));
  return { orders: totals, revenue: totals.reduce((sum, o) => sum + o.total, 0) };
}
