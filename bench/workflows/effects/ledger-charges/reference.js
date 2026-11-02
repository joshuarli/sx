const INVOICES = tools.mcp__tools__payments_invoices;
const CHARGES = tools.mcp__tools__payments_charges;
const CHARGE = tools.mcp__tools__payments_charge;

const invoices = (await INVOICES({})).structuredContent.result;
const charged = [];
for (const invoice of invoices.filter((i) => i.status === "open")) {
  const request = { invoice: invoice.id, amount_cents: invoice.amount_cents };
  let answer = await CHARGE(request);
  if (answer.isError) {
    const held = (await CHARGES({ invoice: invoice.id })).structuredContent.result;
    if (held.length === 0) answer = await CHARGE(request);
  }
  charged.push(invoice.id);
}
return charged;
