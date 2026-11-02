export default async function main(api: Api) {
  const start = (await api.sync.getVersion({}))[0]!.version;
  const delays = [40, 80];
  const fetchPage = async (since: number) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return await api.changes.page({ since, limit: 10 });
      } catch (error) {
        const kind = (error as CallError).kind;
        if ((kind !== "Unavailable" && kind !== "RateLimited") || attempt === delays.length) return null;
        await api.sleep(delays[attempt]!);
      }
    }
  };
  let since = start;
  let pages = 0;
  let upserted = 0;
  let discontinued = 0;
  let completed = false;
  for (;;) {
    const page = await fetchPage(since);
    if (page === null) break;
    const kept = page.changes.filter((c) => !page.changes.some((d) => d.sku === c.sku && d.seq > c.seq));
    for (const change of kept) {
      if (change.op === "upsert") {
        await api.products.upsert({
          sku: change.sku,
          name: change.name ?? "",
          price_cents: change.price_cents ?? 0,
          stock: change.stock ?? 0,
        });
        upserted++;
      } else {
        await api.products.discontinue({ sku: change.sku });
        discontinued++;
      }
    }
    await api.sync.setVersion({ version: page.version });
    pages++;
    since = page.version;
    if (!page.more) {
      completed = true;
      break;
    }
  }
  const version = (await api.sync.getVersion({}))[0]!.version;
  const active = (await api.products.activeCount({}))[0]!.n;
  return { since: start, pages, applied: upserted + discontinued, upserted, discontinued, version, active, completed };
}
