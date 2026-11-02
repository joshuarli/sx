export default async function main(api) {
  const tickets = await api.tickets.list({ status: "open" });
  const index = {};
  for (const ticket of tickets) {
    if (ticket.severity !== "high" && ticket.severity !== "critical") continue;
    (index[ticket.assignee] ??= []).push(ticket.id);
  }
  for (const ids of Object.values(index)) ids.sort();
  return index;
}
