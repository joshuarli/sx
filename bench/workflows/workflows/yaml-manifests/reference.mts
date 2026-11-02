const SCALAR = /^(\s+)(version|owner|tag|replicas):[ \t]*("[^"]*"|'[^']*'|[^\s#]*)/;

type Located = { line: number; value: string; quote: string };

function locate(lines: string[], section: string, key: string): Located | null {
  let inside = false;
  let inner = false;
  for (let at = 0; at < lines.length; at++) {
    const line = lines[at]!.replace(/\r$/, "");
    if (/^\S/.test(line)) {
      inside = line.startsWith(`${section}:`);
      inner = false;
      continue;
    }
    if (!inside) continue;
    if (section === "spec" && key === "tag") {
      if (/^ {2}image:/.test(line)) inner = true;
      else if (/^ {2}\S/.test(line)) inner = false;
      if (!inner) continue;
    } else if (!/^ {2}\S/.test(line)) {
      continue;
    }
    const found = SCALAR.exec(line);
    if (found !== null && found[2] === key) {
      const raw = found[3]!;
      const quote = raw.startsWith('"') || raw.startsWith("'") ? raw[0]! : "";
      return { line: at, value: quote === "" ? raw : raw.slice(1, -1), quote };
    }
  }
  return null;
}

function replace(lines: string[], found: Located, value: string): void {
  const text = lines[found.line]!;
  const raw = `${found.quote}${found.value}${found.quote}`;
  const at = text.indexOf(raw, text.indexOf(":"));
  lines[found.line] = text.slice(0, at) + `${found.quote}${value}${found.quote}` + text.slice(at + raw.length);
}

export default async function main(api: Api) {
  const dir = "{{root}}/manifests";
  const entries = await api.fs.list({ path: dir });
  const names = entries
    .filter((entry) => entry.kind === "File" && entry.name.endsWith(".yaml") && entry.name !== "legacy.yaml")
    .map((entry) => entry.name)
    .sort();
  const updated: { file: string; version: string }[] = [];
  const skipped: string[] = [];
  for (const file of names) {
    const source = await api.fs.readText({ path: `${dir}/${file}` });
    const lines = source.split("\n");
    const owner = locate(lines, "metadata", "owner");
    if (owner !== null && owner.value === "frozen") {
      skipped.push(file);
      continue;
    }
    const version = locate(lines, "metadata", "version")!;
    const parts = version.value.split(".");
    const next = `${parts[0]}.${parts[1]}.${Number(parts[2]) + 1}`;
    replace(lines, version, next);
    const tag = locate(lines, "spec", "tag");
    if (tag !== null && tag.value === version.value) replace(lines, tag, next);
    const replicas = locate(lines, "spec", "replicas");
    if (replicas !== null && Number(replicas.value) < 2) replace(lines, replicas, "2");
    await api.fs.writeText({ path: `${dir}/${file}`, text: lines.join("\n") });
    updated.push({ file, version: next });
  }
  return { updated, skipped };
}
