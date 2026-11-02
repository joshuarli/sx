const READ = tools.mcp__tools__fs_read_text;
const LIST = tools.mcp__tools__fs_list;
const MOVE = tools.mcp__tools__fs_move;

const deploy = "{{root}}/deploy";
const contents = async (name) => (await READ({ path: `${deploy}/${name}` })).content[0].text;
const names = async (directory) => (await LIST({ path: `${deploy}/${directory}` })).structuredContent.result.map((e) => e.name);

const listed = JSON.parse(await contents("retire.json")).retire;
const releases = JSON.parse(await contents("releases.json"));
const pinned = (await contents("pins.txt")).split("\n").map((line) => line.trim().split(" ")[0]).filter((word) => word !== "");
const protectedNames = new Set([...pinned, releases.live, releases.staging]);
const present = new Set(await names("builds"));
const archived = new Set(await names("archive"));

const moved = [];
const skipped = [];
for (const name of listed) {
  if (protectedNames.has(name) || !present.has(name) || archived.has(name)) {
    skipped.push(name);
    continue;
  }
  const answer = await MOVE({ from: `${deploy}/builds/${name}`, to: `${deploy}/archive/${name}` });
  if (answer.isError) throw new Error(`move ${name} failed: ${answer.content[0].text}`);
  moved.push(name);
}
return { moved, skipped };
