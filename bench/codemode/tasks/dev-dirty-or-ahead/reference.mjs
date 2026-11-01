export default async function main(api) {
  const repos = await api.catalog.list({ tags: [] });
  const statuses = await Promise.all(repos.map((repo) => api.git.status({ repo: repo.id })));
  return statuses
    .filter((s) => s.dirty || (s.ahead ?? 0) > 0)
    .map((s) => ({ repo: s.repo, ahead: s.ahead ?? 0 }))
    .sort((a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0));
}
