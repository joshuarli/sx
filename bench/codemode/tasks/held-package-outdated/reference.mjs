const INSTALLED = { "left-pad": "1.2.0", chalk: "4.1.2", ms: "2.1.3", "ghost-pkg": "0.0.1", debug: "4.3.4" };

export default async function main(api) {
  const outdated = [];
  const missing = [];
  await Promise.all(
    Object.entries(INSTALLED).map(async ([name, installed]) => {
      let versions;
      try {
        versions = await api.registry.versions({ name });
      } catch (error) {
        if (error.kind !== "NotFound") throw error;
        missing.push(name);
        return;
      }
      let latest = null;
      for (const text of versions) {
        const v = api.semver.parse(text);
        if (v && (latest === null || api.semver.compare(v, latest) > 0)) latest = v;
      }
      const current = api.semver.parse(installed);
      if (latest && api.semver.compare(latest, current) > 0) {
        const bump = latest.major !== current.major ? "major" : latest.minor !== current.minor ? "minor" : "patch";
        outdated.push({ name, installed, latest: api.semver.show(latest), bump });
      }
    }),
  );
  return { outdated: outdated.sort((a, b) => (a.name < b.name ? -1 : 1)), missing: missing.sort() };
}
