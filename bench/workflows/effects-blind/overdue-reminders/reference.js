const INVOICES = tools.mcp__tools__billing_invoices;
const CUSTOMER = tools.mcp__tools__billing_customer;
const EMAIL = tools.mcp__tools__billing_email;

const overdue = (await INVOICES({})).structuredContent.result.filter((i) => i.status === "overdue");
const byCustomer = new Map();
for (const invoice of overdue) {
  if (!byCustomer.has(invoice.customer_id)) byCustomer.set(invoice.customer_id, []);
  byCustomer.get(invoice.customer_id).push(invoice.id);
}
const reminded = [];
for (const [id, names] of byCustomer) {
  const customer = (await CUSTOMER({ id })).structuredContent;
  if (customer.do_not_contact) continue;
  const sent = await EMAIL({ to: customer.email, subject: "Overdue invoices", body: `These invoices are overdue: ${names.join(", ")}` });
  if (sent.isError) throw new Error(`email to ${id} failed`);
  reminded.push(id);
}
return reminded;
