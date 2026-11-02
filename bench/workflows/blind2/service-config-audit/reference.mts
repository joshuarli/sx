type Entry = { name: string; kind: string };

export default async function main(api: Api) {
  const base = "{{root}}/services";
  const skip = (name: string) => name.startsWith("_") || name.startsWith(".");
  const services: string[] = [];
  let frontier = [""];
  for (let depth = 0; depth < 4 && frontier.length > 0; depth++) {
    const listed = await Promise.all(
      frontier.map(async (dir) => ({ dir, entries: (await api.fs.list({ path: dir === "" ? base : `${base}/${dir}` })) as Entry[] })),
    );
    const next: string[] = [];
    for (const { dir, entries } of listed) {
      if (dir !== "" && entries.some((e) => e.name === "service.json" && e.kind === "File")) {
        services.push(dir);
        continue;
      }
      for (const entry of entries) {
        if (entry.kind === "Directory" && !skip(entry.name)) next.push(dir === "" ? entry.name : `${dir}/${entry.name}`);
      }
    }
    frontier = next;
  }
  type Parsed = { path: string; value: Record<string, unknown> | null };
  const parsed: Parsed[] = await Promise.all(
    services.map(async (path) => {
      const text = await api.fs.readText({ path: `${base}/${path}/service.json` });
      try {
        const value: unknown = JSON.parse(text);
        return { path, value: typeof value === "object" && value !== null && !Array.isArray(value) ? (value as Record<string, unknown>) : {} };
      } catch {
        return { path, value: null };
      }
    }),
  );
  const portOf = (value: Record<string, unknown>) => {
    const port = value["port"];
    return typeof port === "number" && Number.isInteger(port) && port >= 1024 && port <= 65535 ? port : null;
  };
  const ports = parsed.flatMap((p) => (p.value === null ? [] : [portOf(p.value)])).filter((port) => port !== null);
  const httpsString = (value: unknown) => typeof value === "string" && value.startsWith("https://");
  const audited = parsed.map(({ path, value }) => {
    if (value === null) return { path, status: "invalid", findings: [] as string[] };
    const port = portOf(value);
    const repository = value["repository"];
    const checks: [string, boolean][] = [
      ["name", value["name"] !== path.split("/").at(-1)],
      ["version", typeof value["version"] !== "string" || !/^\d+\.\d+\.\d+$/.test(value["version"])],
      ["port", port === null],
      ["port-conflict", port !== null && ports.filter((q) => q === port).length > 1],
      [
        "repository",
        !(
          httpsString(repository) ||
          (typeof repository === "object" && repository !== null && httpsString((repository as Record<string, unknown>)["url"]))
        ),
      ],
      ["owner", typeof value["owner"] !== "string" || value["owner"] === ""],
    ];
    const findings = checks.filter(([, broken]) => broken).map(([name]) => name);
    return { path, status: findings.length === 0 ? "ok" : "fail", findings };
  });
  audited.sort((a, b) => (a.path < b.path ? -1 : a.path > b.path ? 1 : 0));
  const text = audited.map((a) => `${a.path}\t${a.status}\t${a.findings.join(",")}\n`).join("");
  await api.fs.writeText({ path: "{{root}}/audit.tsv", text });
  const tally = new Map<string, number>();
  for (const a of audited) for (const f of a.findings) tally.set(f, (tally.get(f) ?? 0) + 1);
  const findings = [...tally]
    .map(([finding, count]) => ({ finding, count }))
    .sort((a, b) => b.count - a.count || (a.finding < b.finding ? -1 : a.finding > b.finding ? 1 : 0));
  const paths = (status: string) => audited.filter((a) => a.status === status).map((a) => a.path);
  return {
    services: audited.length,
    ok: paths("ok").length,
    fail: paths("fail").length,
    invalid: paths("invalid").length,
    invalid_paths: paths("invalid"),
    findings,
  };
}
