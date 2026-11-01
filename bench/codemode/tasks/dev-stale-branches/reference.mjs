export default async function main(api) {
  const repos = await api.catalog.list({ tags: [] });
  const found = await Promise.all(
    repos.map(async (repo) => {
      const status = await api.git.status({ repo: repo.id });
      if (status.branch === null) return null;
      const remote = await api.github.repo({ repo: repo.id });
      return status.branch === remote.branch ? null : `${repo.id}@${status.branch}`;
    }),
  );
  return found.filter((entry) => entry !== null).sort();
}
