const QUEUES = tools.mcp__tools__jobs_queues;
const LIST = tools.mcp__tools__jobs_list;
const WRITE = tools.mcp__tools__fs_write_text;

const queues = new Map((await QUEUES({})).structuredContent.result.map((q) => [q.name, q]));
const jobs = [];
let cursor;
do {
  let page;
  for (let attempt = 0; ; attempt++) {
    const answer = await LIST(cursor === undefined ? {} : { cursor });
    if (!answer.isError) {
      page = answer.structuredContent;
      break;
    }
    if (attempt === 3) throw new Error("the job service kept failing");
    await new Promise((resolve) => setTimeout(resolve, 200));
  }
  jobs.push(...page.items);
  cursor = page.next ?? undefined;
} while (cursor !== undefined);
const stuck = jobs.filter((j) => {
  const queue = queues.get(j.queue);
  return queue !== undefined && (j.state === "queued" || j.state === "running") && j.age_s > queue.max_age_s;
});
const owners = [...new Set(stuck.map((j) => queues.get(j.queue).owner))].sort();
let report = "";
for (const owner of owners) {
  const own = stuck
    .filter((j) => queues.get(j.queue).owner === owner)
    .sort((a, b) => b.age_s - a.age_s || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  report += `${owner}: ${own.length}\n`;
  for (const j of own) report += `  ${j.id} ${j.queue} ${j.state} ${j.age_s}\n`;
}
report += `total: ${stuck.length}\n`;
await WRITE({ path: "{{root}}/reports/stuck.txt", text: report });
return stuck.length;
