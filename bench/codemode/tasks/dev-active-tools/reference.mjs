export default async function main(api) {
  const repos = await api.catalog.list({ tags: ["tool"] });
  return repos
    .filter((repo) => !repo.archived)
    .map((repo) => repo.name)
    .sort();
}
