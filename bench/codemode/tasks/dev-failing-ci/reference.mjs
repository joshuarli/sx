export default async function main(api) {
  const repos = await api.catalog.list({ tags: ["service"] });
  const states = await Promise.all(
    repos.map(async (repo) => {
      const remote = await api.github.repo({ repo: repo.id });
      const ci = await api.github.ci({ repo: repo.id, sha: remote.sha });
      return { id: repo.id, state: ci.state };
    }),
  );
  return states.filter((s) => s.state === "Failing").map((s) => s.id).sort();
}
