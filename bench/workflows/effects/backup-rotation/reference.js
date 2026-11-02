const root = "{{root}}/backups";
const LIST = tools.mcp__tools__fs_list;
const DELETE = tools.mcp__tools__fs_delete;
const list = async (path) => (await LIST({ path })).structuredContent.result;

const services = (await list(root)).filter((e) => e.kind === "Directory").map((e) => e.name);
const deleted = {};
for (const service of services) {
  const entries = await list(`${root}/${service}`);
  const locked = entries.some((e) => e.name === ".lock");
  const backups = entries
    .filter((e) => e.kind === "File" && /^[a-z]+-\d{4}-\d{2}-\d{2}\.tar\.gz$/.test(e.name))
    .map((e) => e.name)
    .sort();
  deleted[service] = locked ? [] : backups.slice(0, Math.max(0, backups.length - 3));
  for (const name of deleted[service]) await DELETE({ path: `${root}/${service}/${name}` });
}
return deleted;
