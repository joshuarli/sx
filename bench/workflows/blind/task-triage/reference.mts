export default async function main(api: Api) {
  const engineers = ["ines", "omar", "priya", "tariq"];
  const tasks: { id: string; labels: string[]; priority: string; assignee: string | null; status: string }[] = [];
  let pages = 0;
  for (let cursor: string | null = null; ; ) {
    const page = await api.mcpTracker.listTasks({ cursor });
    pages++;
    tasks.push(...page.tasks);
    if (page.next_cursor === null) break;
    cursor = page.next_cursor;
  }
  const number = (id: string) => Number(id.replace("T-", ""));
  const eligible = tasks
    .filter((t) => t.labels.includes("bug") && t.status === "open" && t.assignee === null)
    .sort((a, b) => number(a.id) - number(b.id));
  const loads = engineers.map((e) => tasks.filter((t) => t.status !== "done" && t.assignee === e).length);
  const triaged: { id: string; assignee: string }[] = [];
  const failed: string[] = [];
  for (const t of eligible) {
    let idx = 0;
    for (let i = 1; i < engineers.length; i++) if (loads[i]! < loads[idx]!) idx = i;
    const name = engineers[idx]!;
    const p0 = t.priority === "p0";
    try {
      await api.mcpTracker.assignTask({ id: t.id, assignee: name, status: p0 ? "in-progress" : null });
    } catch {
      failed.push(t.id);
      continue;
    }
    if (p0) await api.mcpTracker.addComment({ id: t.id, text: `Triaged to ${name} (p0)` });
    loads[idx]!++;
    triaged.push({ id: t.id, assignee: name });
  }
  return {
    pages,
    seen: tasks.length,
    triaged,
    failed,
    load: engineers.map((engineer, i) => ({ engineer, load: loads[i]! })),
  };
}
