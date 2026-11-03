const LIST = tools.mcp__tools__fs_list;
const READ = tools.mcp__tools__fs_read_text;
const WRITE = tools.mcp__tools__fs_write_text;
const STAFF = tools.mcp__tools__hr_staff;

const rosters = "{{root}}/rosters";
const staff = new Map();
let cursor;
do {
  let page;
  for (let attempt = 0; ; attempt++) {
    const answer = await STAFF(cursor === undefined ? {} : { cursor });
    if (!answer.isError) {
      page = answer.structuredContent;
      break;
    }
    if (attempt === 3) throw new Error("the directory kept failing");
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  for (const person of page.items) staff.set(person.id, person);
  cursor = page.next ?? undefined;
} while (cursor !== undefined);
const files = (await LIST({ path: rosters })).structuredContent.result.filter(
  (e) => e.kind === "File" && /^week-\d+\.csv$/.test(e.name),
);
const overtime = new Map();
for (const file of files) {
  const lines = (await READ({ path: `${rosters}/${file.name}` })).content[0].text.split("\n");
  const worked = new Map();
  for (const line of lines) {
    const found = /^(\w+),\w+,(\d+):(\d+),(\d+):(\d+),(\d+)\s*$/.exec(line);
    if (!found) continue;
    const start = Number(found[2]) * 60 + Number(found[3]);
    const end = Number(found[4]) * 60 + Number(found[5]);
    const length = end < start ? end - start + 1440 : end - start;
    worked.set(found[1], (worked.get(found[1]) ?? 0) + length - Number(found[6]));
  }
  for (const [id, minutes] of worked) {
    const person = staff.get(id);
    if (!person) continue;
    overtime.set(id, (overtime.get(id) ?? 0) + Math.max(0, minutes - person.contract_minutes));
  }
}
const due = [...overtime.entries()]
  .filter(([, minutes]) => minutes > 0)
  .map(([id, minutes]) => ({ name: staff.get(id).name, team: staff.get(id).team, minutes }))
  .sort((a, b) => b.minutes - a.minutes || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
const report = "name,team,overtime_min\n" + due.map((d) => `${d.name},${d.team},${d.minutes}\n`).join("");
await WRITE({ path: "{{root}}/out/overtime.csv", text: report });
return due.length;
