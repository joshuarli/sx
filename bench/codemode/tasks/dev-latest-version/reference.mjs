export default async function main(api) {
  const versions = await api.registry.versions({ name: "left-pad" });
  let best = null;
  for (const raw of versions) {
    const version = api.semver.parse(api.text.trim(raw));
    if (version && (best === null || api.semver.compare(version, best) > 0)) best = version;
  }
  return best === null ? null : api.semver.show(best);
}
