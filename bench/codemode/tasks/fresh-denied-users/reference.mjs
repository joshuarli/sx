async function readPage(api, page) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await api.logs.page({ page });
    } catch (error) {
      if (error.kind !== "RateLimited" || attempt === 4) throw error;
      await api.sleep(1000);
    }
  }
}

export default async function main(api) {
  const denied = new Map();
  for (let page = 1; page <= 20; page++) {
    const result = await readPage(api, page);
    for (const event of result.events.filter((e) => e.outcome === "denied")) denied.set(event.user, (denied.get(event.user) ?? 0) + 1);
    if (result.last) break;
  }
  return [...denied].filter(([, n]) => n >= 2).map(([user, n]) => ({ user, denied: n })).sort((a, b) => b.denied - a.denied || (a.user < b.user ? -1 : 1));
}
