export default async function main(api) {
  const repos = await api.catalog.list({ tags: [] });
  const counts = new Map();
  for (const repo of repos) for (const tag of repo.tags) counts.set(tag, (counts.get(tag) ?? 0) + 1);
  return [...counts]
    .map(([tag, count]) => ({ tag, count }))
    .sort((a, b) => b.count - a.count || (a.tag < b.tag ? -1 : 1));
}
