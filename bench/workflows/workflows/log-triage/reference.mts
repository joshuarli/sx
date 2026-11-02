export default async function main(api: Api) {
  const dir = "{{root}}/logs";
  const entries = await api.fs.list({ path: dir });
  const names = entries
    .filter((e) => e.kind === "File" && e.name.endsWith(".log"))
    .map((e) => e.name)
    .sort();
  const texts = await Promise.all(names.map((name) => api.fs.readText({ path: `${dir}/${name}` })));
  const codes = new Map<string, number>();
  const services = new Map<string, number>();
  let errors = 0;
  for (const text of texts) {
    for (const line of text.split("\n")) {
      const fields = line.replaceAll("\r", "").split(" ");
      if (fields.length >= 4 && fields[1] === "ERROR") {
        errors++;
        codes.set(fields[3]!, (codes.get(fields[3]!) ?? 0) + 1);
        services.set(fields[2]!, (services.get(fields[2]!) ?? 0) + 1);
      }
    }
  }
  const ordered = (counts: Map<string, number>) =>
    [...counts].sort((a, b) => b[1] - a[1] || (a[0] < b[0] ? -1 : a[0] > b[0] ? 1 : 0));
  const byCode = ordered(codes);
  const byService = ordered(services);
  const row = ([name, count]: [string, number]) => `${name}: ${count}`;
  const lines = [
    `files: ${names.length}`,
    `errors: ${errors}`,
    "by code:",
    ...byCode.map(row),
    "by service:",
    ...byService.map(row),
  ];
  await api.fs.writeText({ path: "{{root}}/summary.txt", text: lines.join("\n") + "\n" });
  return {
    files: names.length,
    errors,
    by_code: byCode.map(([code, count]) => ({ code, count })),
    by_service: byService.map(([service, count]) => ({ service, count })),
  };
}
