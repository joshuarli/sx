export default async function main(api) {
  const repos = await api.catalog.list({ tags: [] });
  const byTag = new Map();
  for (const repo of repos) {
    for (const raw of repo.tags) {
      const tag = api.text.casefold(api.text.trim(raw));
      if (api.text.length(tag) === 0) continue;
      if (!byTag.has(tag)) byTag.set(tag, new Set());
      byTag.get(tag).add(repo.id);
    }
  }
  return [...byTag.entries()]
    .filter(([, ids]) => ids.size >= 2)
    .map(([tag, ids]) => ({ tag, repos: [...ids].sort() }))
    .sort((a, b) => (a.tag < b.tag ? -1 : 1));
}
