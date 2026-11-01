const byRepo = (a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0);

export default async function main(api) {
  const repos = await api.catalog.list({ tags: ["released"] });
  const results = await Promise.all(
    repos.map(async (repo) => {
      try {
        const tags = await api.github.listTags({ repo: repo.id });
        let best = null;
        for (const tag of tags) {
          const version = api.semver.parse(api.text.startsWith(tag, "v") ? tag.slice(1) : tag);
          if (version && (best === null || api.semver.compare(version, best) > 0)) best = version;
        }
        return { repo: repo.id, latest: best ? api.semver.show(best) : null };
      } catch (error) {
        return { repo: repo.id, kind: error.kind };
      }
    }),
  );
  return {
    releases: results.filter((r) => "latest" in r).sort(byRepo),
    failed: results.filter((r) => "kind" in r).sort(byRepo),
  };
}
