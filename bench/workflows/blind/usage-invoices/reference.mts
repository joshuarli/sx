type Event = { metric: string; quantity: number };

export default async function main(api: Api) {
  const accounts: { id: string; plan: string }[] = [];
  for (let page = 1; ; page++) {
    const answer = await api.billing.accounts({ page });
    accounts.push(...answer.accounts);
    if (page >= answer.pages) break;
  }
  const recorded = new Set((await api.ledger.all({})).map((r) => r.account));
  const ledgered = accounts.filter((a) => recorded.has(a.id));
  const todo = accounts.filter((a) => a.plan !== "free" && !recorded.has(a.id));
  const fetchUsage = async (a: { id: string; plan: string }) => {
    for (let attempt = 0; ; attempt++) {
      try {
        const u = await api.billing.usage({ account: a.id });
        return { ...a, state: "ok", events: u.events as Event[] };
      } catch (error) {
        const kind = (error as CallError).kind;
        if (kind === "NotFound") return { ...a, state: "unknown", events: [] as Event[] };
        if (kind !== "Unavailable" || attempt === 2) return { ...a, state: "failed", events: [] as Event[] };
        await api.sleep(50);
      }
    }
  };
  const usage: Awaited<ReturnType<typeof fetchUsage>>[] = [];
  let next = 0;
  const worker = async () => {
    while (next < todo.length) usage.push(await fetchUsage(todo[next++]!));
  };
  await Promise.all([worker(), worker(), worker(), worker(), worker()]);
  const sum = (events: Event[], metric: string) =>
    events.filter((e) => e.metric === metric).reduce((total, e) => total + e.quantity, 0);
  const subtotal = (plan: string, events: Event[]) => {
    const used = sum(events, "api");
    const storage = sum(events, "storage");
    if (plan === "standard") {
      return Math.max(Math.min(used, 10000) - 1000, 0) * 3 + Math.max(used - 10000, 0) * 2 + storage * 50;
    }
    const raw = Math.max(used - 10000, 0) + storage * 30;
    return raw > 0 && raw < 5000 ? 5000 : raw;
  };
  const priced = usage
    .filter((u) => u.state === "ok")
    .sort((a, b) => (a.id < b.id ? -1 : 1))
    .map((u) => {
      const base = subtotal(u.plan, u.events);
      return { id: u.id, base, cents: base + Math.floor((base * 7 + 50) / 100) };
    });
  const noCharge = priced.filter((p) => p.base === 0);
  const invoiced: { account: string; cents: number; invoice: string }[] = [];
  const already: string[] = [];
  const failed = usage.filter((u) => u.state === "failed").map((u) => u.id);
  for (const p of priced.filter((q) => q.base > 0)) {
    try {
      let made;
      for (let attempt = 0; ; attempt++) {
        try {
          made = await api.billing.invoice({ account: p.id, cents: p.cents });
          break;
        } catch (error) {
          if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
          await api.sleep(100);
        }
      }
      await api.ledger.record({ account: p.id, invoice: made.invoice, cents: p.cents });
      invoiced.push({ account: p.id, cents: p.cents, invoice: made.invoice });
    } catch (error) {
      if ((error as CallError).kind === "Conflict") already.push(p.id);
      else failed.push(p.id);
    }
  }
  const ids = (xs: { id: string }[]) => xs.map((x) => x.id).sort();
  return {
    accounts: accounts.length,
    free: accounts.filter((a) => a.plan === "free").length,
    ledgered: ids(ledgered),
    unknown: ids(usage.filter((u) => u.state === "unknown")),
    no_charge: ids(noCharge),
    already: already.sort(),
    failed: failed.sort(),
    invoiced,
    total_cents: invoiced.reduce((total, i) => total + i.cents, 0),
  };
}
