export default async function main(api) {
  const repos = await api.catalog.list({ tags: ["tool"] });
  const results = await Promise.all(
    repos.map(async (repo) => {
      try {
        const ci = await api.github.latestCi({ repo: repo.id });
        return { id: repo.id, state: ci.state };
      } catch {
        return { id: repo.id, state: null };
      }
    }),
  );
  return {
    failing: results.filter((r) => r.state === "Failing").map((r) => r.id).sort(),
    unknown: results.filter((r) => r.state === null).map((r) => r.id).sort(),
  };
}
