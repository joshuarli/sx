export default async function main(api) {
  const outdated = [];
  for (const dep of await api.deps.installed({ app: "portal" })) {
    const installed = api.semver.parse(dep.version);
    let latest = null;
    for (const raw of await api.registry.versions({ name: dep.name })) {
      const version = api.semver.parse(api.text.trim(raw));
      if (version && (latest === null || api.semver.compare(version, latest) > 0)) latest = version;
    }
    if (latest !== null && api.semver.compare(latest, installed) > 0) {
      outdated.push({ name: dep.name, installed: dep.version, latest: api.semver.show(latest) });
    }
  }
  return outdated;
}
