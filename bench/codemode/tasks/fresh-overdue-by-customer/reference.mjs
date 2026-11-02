export default async function main(api) {
  const totals = new Map();
  for (let offset = 0; ; offset += 2) {
    const page = await api.invoices.page({ offset, limit: 2 });
    for (const invoice of page.filter((i) => i.status === "overdue")) {
      totals.set(invoice.customer, (totals.get(invoice.customer) ?? 0) + invoice.amount);
    }
    if (page.length < 2) break;
  }
  return [...totals].map(([customer, total]) => ({ customer, total })).sort((a, b) => b.total - a.total || (a.customer < b.customer ? -1 : 1));
}
