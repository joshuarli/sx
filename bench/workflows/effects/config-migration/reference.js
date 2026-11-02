const root = "{{root}}";
const READ = tools.mcp__tools__fs_read_text;
const STAT = tools.mcp__tools__fs_stat;
const MAKE_DIRECTORY = tools.mcp__tools__fs_make_directory;
const MOVE = tools.mcp__tools__fs_move;
const DELETE = tools.mcp__tools__fs_delete;
const WRITE = tools.mcp__tools__fs_write_text;
const readText = async (path) => (await READ({ path })).content[0].text;

const mapping = JSON.parse(await readText(`${root}/conf/mapping.json`));
const services = new Map();
for (const name of Object.keys(mapping).sort()) {
  const service = name.split("-")[0];
  const old = `${root}/conf/old/${name}`;
  const target = `${root}/${mapping[name]}`;
  let state = "free";
  if ((await STAT({ path: target })).structuredContent.result !== null) {
    state = (await readText(old)) === (await readText(target)) ? "same" : "different";
  }
  if (!services.has(service)) services.set(service, []);
  services.get(service).push({ name, old, target, state });
}

const migrated = {};
const conflicts = [];
for (const [service, entries] of services) {
  if (entries.some((e) => e.state === "different")) {
    conflicts.push(service);
    continue;
  }
  await MAKE_DIRECTORY({ path: `${root}/conf/new/${service}` });
  for (const e of entries) {
    if (e.state === "same") await DELETE({ path: e.old });
    else await MOVE({ from: e.old, to: e.target });
  }
  migrated[service] = entries.map((e) => e.name);
}
await WRITE({ path: `${root}/conf/new/MIGRATED.json`, text: JSON.stringify(migrated) });
return conflicts.sort();
