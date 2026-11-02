export default async function main(api: Api) {
  const root = "{{root}}/project";
  const entries = new Set(
    (await api.fs.readText({ path: `${root}/.tidyignore` }))
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => line !== "" && !line.startsWith("#")),
  );
  const considered: string[] = [];
  for (const dir of ["src", "docs"]) {
    for (const entry of await api.fs.list({ path: `${root}/${dir}` })) {
      if (entry.kind === "File" && /\.(md|txt|py)$/.test(entry.name)) considered.push(`${dir}/${entry.name}`);
    }
  }
  considered.sort();
  const ignored = considered.filter((path) => entries.has(path));
  const targets = considered.filter((path) => !entries.has(path));
  const tidy = (text: string) => {
    const lines = text
      .replaceAll("\r\n", "\n")
      .replace(/[ \t]+$/gm, "")
      .split("\n")
      .map((line) => line.replace(/^\t+/, (tabs) => "    ".repeat(tabs.length)));
    const body = lines.join("\n").replace(/\n+$/, "");
    return body === "" ? "" : `${body}\n`;
  };
  const read = await Promise.all(
    targets.map(async (path) => {
      try {
        return { path, text: await api.fs.readText({ path: `${root}/${path}` }) };
      } catch {
        return { path, text: null };
      }
    }),
  );
  const skipped = read.filter((r) => r.text === null).map((r) => r.path);
  const readable = read.flatMap((r) => (r.text === null ? [] : [{ path: r.path, text: r.text }]));
  const dirty = readable.filter((r) => tidy(r.text) !== r.text);
  for (const r of dirty) await api.fs.writeText({ path: `${root}/${r.path}`, text: tidy(r.text) });
  return {
    changed: dirty.map((r) => r.path),
    unchanged: readable.length - dirty.length,
    ignored,
    skipped,
    delta: dirty.reduce((sum, r) => sum + tidy(r.text).length - r.text.length, 0),
  };
}
