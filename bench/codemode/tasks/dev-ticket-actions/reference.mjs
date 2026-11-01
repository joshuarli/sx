function action(ticket) {
  if (ticket.priority === "urgent" && ticket.assignee === null) return "page";
  if (ticket.priority === "urgent" || (ticket.priority === "high" && ticket.age > 7)) return "escalate";
  if (ticket.age > 30) return "close-stale";
  return null;
}

export default async function main(api) {
  const tickets = await api.support.openTickets({});
  return tickets.map((t) => ({ id: t.id, action: action(t) })).filter((t) => t.action !== null);
}
