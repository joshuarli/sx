export default async function main(api: Api) {
  await api.sqlite.execute({
    database: "{{root}}/orders.db",
    sql: "CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT NOT NULL, cents INTEGER NOT NULL, status TEXT NOT NULL, note TEXT)",
  });
  const delays = [50, 100, 200];
  const fetchPage = async (cursor: string | null) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return await api.orders.page({ limit: 25, cursor });
      } catch (error) {
        const kind = (error as CallError).kind;
        if ((kind !== "RateLimited" && kind !== "Unavailable") || attempt === delays.length) throw error;
        await api.sleep(delays[attempt]!);
      }
    }
  };
  const items = [];
  let pages = 0;
  for (let cursor: string | null = null; ; ) {
    const page = await fetchPage(cursor);
    pages++;
    items.push(...page.items);
    if (page.next === null) break;
    cursor = page.next;
  }
  for (const item of items) await api.orders.add(item);
  const statuses = await api.orders.summary({});
  return { pages, fetched: items.length, stored: statuses.reduce((sum, row) => sum + row.n, 0), statuses };
}
