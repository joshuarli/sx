export default async function main(api: Api) {
  const first = await api.mcpTaskdesk.listPeople({});
  let people = first.people.map((p) => ({ ...p }));
  const tasks = [];
  for (let cursor: string | null = null; ; ) {
    const page = await api.mcpTaskdesk.listTasks({ cursor });
    tasks.push(...page.items);
    if (page.next_cursor === null) break;
    cursor = page.next_cursor;
  }
  const rank = { urgent: 0, high: 1, normal: 2, low: 3 } as const;
  const todo = tasks
    .filter((t) => t.state === "open" && t.assignee === null)
    .sort(
      (a, b) =>
        rank[a.priority] - rank[b.priority] ||
        b.estimate - a.estimate ||
        (a.id < b.id ? -1 : a.id > b.id ? 1 : 0),
    );
  const assigned: { task: string; assignee: string }[] = [];
  const unassigned: { task: string; reason: "no-skill" | "no-room" | "refused" }[] = [];
  for (const task of todo) {
    const skilled = people.filter((p) => p.skills.includes(task.area));
    const roomy = skilled.filter((p) => p.capacity - p.load >= task.estimate);
    const ordered = [...roomy].sort((a, b) => a.load - b.load || (a.name < b.name ? -1 : a.name > b.name ? 1 : 0));
    let done = false;
    for (const candidate of ordered) {
      try {
        const result = await api.mcpTaskdesk.assignTask({ task_id: task.id, assignee: candidate.name });
        people = people.map((p) => (p.name === candidate.name ? { ...p, load: result.load } : p));
        assigned.push({ task: task.id, assignee: candidate.name });
        done = true;
        break;
      } catch {
        continue;
      }
    }
    if (!done) {
      const reason = skilled.length === 0 ? "no-skill" : roomy.length === 0 ? "no-room" : "refused";
      unassigned.push({ task: task.id, reason });
    }
  }
  const final = await api.mcpTaskdesk.listPeople({});
  return { assigned, unassigned, loads: final.people.map((p) => ({ name: p.name, load: p.load })) };
}
