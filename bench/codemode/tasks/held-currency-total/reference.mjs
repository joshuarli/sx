export default async function main(api) {
  const payments = await api.payments.list();
  const rates = await api.fx.rates({ base: "USD" });
  let total = 0;
  const unconverted = [];
  for (const p of payments) {
    if (p.currency === "USD") total += p.cents;
    else if (p.currency in rates) total += Math.round(p.cents / rates[p.currency]);
    else unconverted.push(p.id);
  }
  return { total, unconverted: unconverted.sort() };
}
