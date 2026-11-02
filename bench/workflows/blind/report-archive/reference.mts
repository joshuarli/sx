export default async function main(api: Api) {
  const reports = "{{root}}/reports";
  const archive = "{{root}}/archive";
  const run = (program: string, args: string[]) =>
    api.process.run({ program, args, cwd: "{{root}}", stdin: null });
  const files = (await api.fs.list({ path: reports })).filter((e) => e.kind === "File").map((e) => e.name);
  const pattern = /^([a-z]+)-(\d{4})-(0[1-9]|1[0-2])-(0[1-9]|[12]\d|3[01])\.([a-z]+)$/;
  const found = [];
  const ignored: string[] = [];
  for (const file of files) {
    const g = pattern.exec(file);
    if (g === null) ignored.push(file);
    else found.push({ file, stem: `${g[1]}-${g[2]}-${g[3]}-${g[4]}`, date: `${g[2]}-${g[3]}-${g[4]}`, year: g[2]!, month: g[3]!, ext: g[5]! });
  }
  const old = found
    .filter((p) => p.date < "2025-12-10")
    .sort((a, b) => (a.date < b.date ? -1 : a.date > b.date ? 1 : a.file < b.file ? -1 : a.file > b.file ? 1 : 0));
  const archived: { from: string; to: string }[] = [];
  const duplicates: string[] = [];
  const lines: string[] = [];
  for (const p of old) {
    const dir = `${archive}/${p.year}/${p.month}`;
    const source = `${reports}/${p.file}`;
    await run("/bin/mkdir", ["-p", dir]);
    let name = p.file;
    if ((await api.fs.stat({ path: `${dir}/${name}` })) !== null) {
      if ((await api.fs.readText({ path: source })) === (await api.fs.readText({ path: `${dir}/${name}` }))) {
        await run("/bin/rm", [source]);
        duplicates.push(p.file);
        lines.push(`${p.file} (duplicate)\n`);
        continue;
      }
      for (let n = 1; ; n++) {
        name = `${p.stem}.${n}.${p.ext}`;
        if ((await api.fs.stat({ path: `${dir}/${name}` })) === null) break;
      }
    }
    await run("/bin/mv", [source, `${dir}/${name}`]);
    archived.push({ from: p.file, to: `${p.year}/${p.month}/${name}` });
    lines.push(`${p.file} -> ${p.year}/${p.month}/${name}\n`);
  }
  const index = `${archive}/INDEX.txt`;
  await api.fs.writeText({ path: index, text: (await api.fs.readText({ path: index })) + lines.join("") });
  return { archived, duplicates, kept: found.length - old.length, ignored: ignored.sort() };
}
