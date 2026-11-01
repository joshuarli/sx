export default async function main(api) {
  const repos = await api.catalog.list({ tags: ["service"] });
  const results = await Promise.all(
    repos.map(async (repo) => {
      try {
        return { id: repo.id, checks: await api.ci.listChecks({ repo: repo.id }) };
      } catch (error) {
        return { id: repo.id, checks: null };
      }
    }),
  );
  const red = results
    .filter((r) => r.checks)
    .map((r) => ({
      repo: r.id,
      failing: r.checks
        .filter((c) => typeof c.state === "object" && c.state.tag === "Failing")
        .map((c) => c.name)
        .sort(),
    }))
    .filter((r) => r.failing.length > 0)
    .sort((a, b) => (a.repo < b.repo ? -1 : 1));
  const unreachable = results.filter((r) => !r.checks).map((r) => r.id).sort();
  return { red, unreachable };
}
