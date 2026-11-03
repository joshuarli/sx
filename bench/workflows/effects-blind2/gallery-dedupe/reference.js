const LIST = tools.mcp__tools__fs_list;
const READ = tools.mcp__tools__fs_read_text;
const DELETE = tools.mcp__tools__fs_delete;

const gallery = "{{root}}/gallery";
const entries = async (path) => (await LIST({ path })).structuredContent.result;
const top = await entries(gallery);
const paths = top.filter((e) => e.kind === "File").map((e) => e.name);
for (const directory of top.filter((e) => e.kind === "Directory")) {
  for (const e of await entries(`${gallery}/${directory.name}`)) {
    if (e.kind === "File") paths.push(`${directory.name}/${e.name}`);
  }
}
paths.sort();
const groups = new Map();
for (const path of paths) {
  const text = (await READ({ path: `${gallery}/${path}` })).content[0].text;
  if (!groups.has(text)) groups.set(text, []);
  groups.get(text).push(path);
}
const doomed = [];
for (const copies of groups.values()) {
  if (copies.length < 2) continue;
  const favorite = (p) => p.startsWith("favorites/");
  doomed.push(...(copies.some(favorite) ? copies.filter((p) => !favorite(p)) : copies.slice(1)));
}
doomed.sort();
for (const path of doomed) await DELETE({ path: `${gallery}/${path}` });
return doomed;
