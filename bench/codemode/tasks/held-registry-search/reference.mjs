async function withRetry(call) {
  for (let attempt = 1; ; attempt += 1) {
    try {
      return await call();
    } catch (error) {
      if (error.kind !== "Timeout" || attempt === 3) throw error;
    }
  }
}

export default async function main(api) {
  const hits = [];
  let pages = 0;
  let after = null;
  do {
    const page = await withRetry(() => api.registry.search({ query: "http", after }));
    pages += 1;
    hits.push(...page.items);
    after = page.next;
  } while (after !== null);
  hits.sort((a, b) => b.downloads - a.downloads || (a.name < b.name ? -1 : 1));
  return { total: hits.reduce((s, h) => s + h.downloads, 0), top: hits.slice(0, 3).map((h) => h.name), pages };
}
