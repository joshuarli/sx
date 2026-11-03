const ORDERS = tools.mcp__tools__ship_orders;
const CUSTOMERS = tools.mcp__tools__ship_customers;
const BOOKINGS = tools.mcp__tools__ship_bookings;
const BOOK = tools.mcp__tools__ship_book;

const customers = (await CUSTOMERS({})).structuredContent.result;
const orders = (await ORDERS({})).structuredContent.result;
const blocked = new Set(customers.filter((c) => c.blocked).map((c) => c.id));
const ready = new Map();
for (const order of orders) {
  if (order.status !== "ready" || blocked.has(order.customer)) continue;
  if (!ready.has(order.customer)) ready.set(order.customer, []);
  ready.get(order.customer).push(order);
}
const booked = [];
for (const [customer, own] of ready) {
  const zone = customers.find((c) => c.id === customer).zone;
  for (let start = 0; start < own.length; start += 4) {
    const slice = own.slice(start, start + 4);
    const ids = slice.map((o) => o.id);
    const request = { customer, zone, orders: ids, weight_g: slice.reduce((sum, o) => sum + o.weight_g, 0) };
    const answer = await BOOK(request);
    if (answer.isError) {
      const held = (await BOOKINGS({ customer })).structuredContent.result;
      if (!held.some((b) => b.orders.join() === ids.join())) await BOOK(request);
    }
    booked.push({ customer, orders: ids });
  }
}
return booked;
