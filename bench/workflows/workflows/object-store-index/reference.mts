export default async function main(api: Api) {
  const root = "{{root}}/docs";
  const top = await api.fs.list({ path: root });
  const markdown = (name: string) => name.endsWith(".md");
  const paths: string[] = top.filter((e) => e.kind === "File" && markdown(e.name)).map((e) => e.name);
  for (const dir of top.filter((e) => e.kind === "Directory")) {
    const inner = await api.fs.list({ path: `${root}/${dir.name}` });
    for (const e of inner) if (e.kind === "File" && markdown(e.name)) paths.push(`${dir.name}/${e.name}`);
  }
  paths.sort();
  const files = [];
  for (const path of paths) {
    const body = await api.fs.readText({ path: `${root}/${path}` });
    const stat = await api.fs.stat({ path: `${root}/${path}` });
    const heading = body.split("\n").find((line) => line.startsWith("# "));
    files.push({ path, bytes: stat === null ? 0 : stat.size, title: heading === undefined ? "" : heading.slice(2) });
  }
  const stored = await api.store.put({ key: "docs-index.json", count: files.length, files });
  return { count: files.length, etag: stored.etag };
}
