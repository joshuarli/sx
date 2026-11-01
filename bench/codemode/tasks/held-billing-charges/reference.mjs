export default async function main(api) {
  const customers = (await api.billing.customers()).sort();
  const usages = await Promise.all(customers.map((customer) => api.billing.usage({ customer })));
  const charged = [];
  const skipped = [];
  const failed = [];
  for (const [i, customer] of customers.entries()) {
    const total = usages[i].reduce((sum, u) => sum + u.units * u.rate, 0);
    if (total === 0) {
      skipped.push(customer);
      continue;
    }
    const cents = Math.round(Math.max(total, 10) * 100);
    try {
      const { charge } = await api.billing.charge({ customer, cents });
      charged.push({ customer, cents, charge });
    } catch (error) {
      failed.push({ customer, kind: error.kind });
    }
  }
  return { charged, skipped, failed };
}
