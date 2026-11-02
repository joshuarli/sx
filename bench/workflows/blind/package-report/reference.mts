type Manifest = Record<string, any>;

const compare = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);

export default async function main(api: Api) {
  const base = "{{root}}/packages";
  const dirs = (await api.fs.list({ path: base })).filter((e) => e.kind === "Directory").map((e) => e.name);
  const loaded = await Promise.all(
    dirs.map(async (dir) => {
      const path = `${base}/${dir}/package.json`;
      if ((await api.fs.stat({ path })) === null) return { dir, state: "no_manifest", manifest: {} as Manifest };
      const text = await api.fs.readText({ path });
      let manifest: Manifest;
      try {
        manifest = JSON.parse(text);
      } catch {
        return { dir, state: "invalid", manifest: {} as Manifest };
      }
      return { dir, state: manifest.private === true ? "private" : "ok", manifest };
    }),
  );
  const rows = loaded
    .filter((l) => l.state === "ok")
    .map(({ dir, manifest: m }) => {
      let license = "UNLICENSED";
      if (typeof m.license === "string") license = m.license;
      else if (m.license && typeof m.license === "object") license = m.license.type ?? "UNLICENSED";
      else if (Array.isArray(m.licenses) && m.licenses.length > 0) license = m.licenses[0].type ?? "UNLICENSED";
      let author = "unknown";
      if (typeof m.author === "string") {
        const at = m.author.indexOf(" <");
        author = (at < 0 ? m.author : m.author.slice(0, at)).trim();
      } else if (m.author && typeof m.author === "object") author = m.author.name ?? "unknown";
      return {
        dir,
        name: (m.name ?? dir) as string,
        version: (m.version ?? "") as string,
        license,
        author,
        deps: Object.keys(m.dependencies ?? {}).length,
        dev: Object.keys(m.devDependencies ?? {}).length,
      };
    })
    .sort((a, b) => compare(a.name, b.name) || compare(a.dir, b.dir));
  const quote = (s: string) => (/[,"]/.test(s) ? `"${s.replaceAll('"', '""')}"` : s);
  const lines = rows.map((r) => [quote(r.name), quote(r.version), quote(r.license), quote(r.author), r.deps, r.dev].join(","));
  await api.fs.writeText({ path: "{{root}}/report.csv", text: ["name,version,license,author,deps,dev_deps", ...lines].join("\n") + "\n" });
  const inState = (state: string) => loaded.filter((l) => l.state === state).map((l) => l.dir).sort(compare);
  const counts = new Map<string, number>();
  for (const r of rows) counts.set(r.license, (counts.get(r.license) ?? 0) + 1);
  const licenses = [...counts].map(([license, count]) => ({ license, count })).sort((a, b) => b.count - a.count || compare(a.license, b.license));
  return { rows: rows.length, private: inState("private"), invalid: inState("invalid"), no_manifest: inState("no_manifest"), licenses };
}
