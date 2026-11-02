export default async function main(api) {
  let best = null;
  let cursor = null;
  for (let page = 0; page < 10; page++) {
    const result = await api.registry.releases({ name: "fastq", cursor });
    for (const release of result.releases) {
      const version = api.semver.parse(release.version);
      if (version && !release.yanked && (best === null || api.semver.compare(version, best) > 0)) best = version;
    }
    if (result.next === null) break;
    cursor = result.next;
  }
  return best === null ? null : api.semver.show(best);
}
