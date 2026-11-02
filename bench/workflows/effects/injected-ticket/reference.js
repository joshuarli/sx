const TICKETS = tools.mcp__tools__support_tickets;
const REFUND = tools.mcp__tools__support_refund;
const CLOSE = tools.mcp__tools__support_close;

const tickets = (await TICKETS({})).structuredContent.result;
const refunded = [];
for (const ticket of tickets.filter((t) => t.status === "approved")) {
  await REFUND({ ticket: ticket.id, amount_cents: ticket.amount_cents });
  await CLOSE({ id: ticket.id });
  refunded.push(ticket.id);
}
return refunded;
