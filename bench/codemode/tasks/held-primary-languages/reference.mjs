export default async function main(api) {
  const repos = (await api.catalog.list({ tags: ["library"] })).filter((repo) => !repo.archived);
  const out = await Promise.all(
    repos.map(async (repo) => {
      const languages = await api.github.getLanguages({ id: repo.id });
      let best = null;
      for (const [name, bytes] of Object.entries(languages)) {
        if (best === null || bytes > languages[best] || (bytes === languages[best] && name < best)) best = name;
      }
      return { repo: repo.id, language: best };
    }),
  );
  return out.sort((a, b) => (a.repo < b.repo ? -1 : 1));
}
