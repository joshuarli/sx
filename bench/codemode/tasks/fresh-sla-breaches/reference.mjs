const ALLOWANCE = { 1: 30, 2: 120, 3: 480 };

export default async function main(api) {
  const now = await api.desk.now();
  const breached = [];
  for (const ticket of await api.desk.tickets()) {
    const elapsed = (ticket["first-response"] ?? now) - ticket.opened;
    const overshoot = elapsed - ALLOWANCE[ticket.priority];
    if (overshoot > 0) breached.push({ id: ticket.id, overshoot });
  }
  return breached.sort((a, b) => b.overshoot - a.overshoot || (a.id < b.id ? -1 : 1));
}
