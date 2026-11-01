const byRepo = (a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0);

export default async function main(api) {
  const repos = await api.catalog.list({ tags: [] });
  const metas = await Promise.all(repos.map((repo) => api.github.getRepo({ id: repo.id })));
  return metas
    .filter((meta) => meta.fork && meta.pushedAt < "2025-01-01")
    .map((meta) => ({ repo: meta.id, upstream: meta.parent }))
    .sort(byRepo);
}
