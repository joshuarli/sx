type Version = [number, number, number];

function parse(text: string): Version | null {
  const match = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/.exec(text);
  return match === null ? null : [Number(match[1]), Number(match[2]), Number(match[3])];
}

function compare(a: Version, b: Version): number {
  return a[0] - b[0] || a[1] - b[1] || a[2] - b[2];
}

const KINDS = ["major", "minor", "patch"] as const;

export default async function main(api: Api) {
  const dir = "{{root}}/manifests";
  const entries = await api.fs.list({ path: dir });
  const files = entries
    .filter((e) => e.kind === "File" && e.name.endsWith(".deps"))
    .map((e) => e.name)
    .sort();
  const deps: { project: string; package: string; current: string }[] = [];
  for (const file of files) {
    const text = await api.fs.readText({ path: `${dir}/${file}` });
    for (const line of text.split("\n")) {
      if (line.trim() === "" || line.startsWith("#")) continue;
      const [name, version] = line.split("==");
      deps.push({ project: file.replace(/\.deps$/, ""), package: name!.trim(), current: version!.trim() });
    }
  }
  const packages = [...new Set(deps.map((d) => d.package))].sort();
  const latest = new Map<string, Version | null>();
  const known = new Map<string, boolean>();
  const queue = [...packages];
  const worker = async () => {
    for (let name = queue.shift(); name !== undefined; name = queue.shift()) {
      try {
        const { versions } = await api.registry.versions({ name });
        const valid = versions.map(parse).filter((v): v is Version => v !== null);
        latest.set(name, valid.sort(compare).at(-1) ?? null);
        known.set(name, true);
      } catch {
        latest.set(name, null);
        known.set(name, false);
      }
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  const order = (a: { project: string; package: string }, b: { project: string; package: string }) =>
    a.project < b.project ? -1 : a.project > b.project ? 1 : a.package < b.package ? -1 : a.package > b.package ? 1 : 0;
  const outdated: { project: string; package: string; current: string; latest: string; kind: (typeof KINDS)[number] }[] = [];
  const missing: { project: string; package: string }[] = [];
  for (const dep of deps) {
    if (!known.get(dep.package)) {
      missing.push({ project: dep.project, package: dep.package });
      continue;
    }
    const best = latest.get(dep.package);
    const current = parse(dep.current);
    if (best && current && compare(best, current) > 0) {
      const kind = KINDS[[0, 1, 2].find((i) => best[i] !== current[i])!]!;
      outdated.push({ ...dep, latest: best.join("."), kind });
    }
  }
  const counts = { major: 0, minor: 0, patch: 0 };
  for (const item of outdated) counts[item.kind]++;
  return { outdated: outdated.sort(order), missing: missing.sort(order), counts };
}
