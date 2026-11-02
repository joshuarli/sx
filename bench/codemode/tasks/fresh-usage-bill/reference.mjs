export default async function main(api) {
  const usage = await api.usage.lines({ account: "acme" });
  const rates = new Map((await api.plans.rates()).map((r) => [r.plan, r.rate]));
  const lines = usage.map((l) => ({ plan: l.plan, cost: l.quantity * rates.get(l.plan) }));
  return { lines, total: lines.reduce((sum, l) => sum + l.cost, 0) };
}
