export default async function main(api: Api) {
  const latest = new Map<number, { id: number; title: string; severity: string; state: string }>();
  let pages = 0;
  for (let page = 1; ; page++) {
    const { items } = await api.issues.page({ page, per_page: 20 });
    pages++;
    if (items.length === 0) break;
    for (const item of items) latest.set(item.id, item);
  }
  const candidates = [...latest.values()]
    .filter((i) => i.severity === "critical" && i.state === "open")
    .sort((a, b) => a.id - b.id);
  const known = new Set((await api.notified.ids({})).map((row) => row.issue_id));
  const already: number[] = [];
  const notified: number[] = [];
  const failed: number[] = [];
  for (const issue of candidates) {
    if (known.has(issue.id)) {
      already.push(issue.id);
      continue;
    }
    try {
      await api.chat.post({ channel: "#oncall", text: `Critical issue #${issue.id}: ${issue.title}` });
    } catch {
      failed.push(issue.id);
      continue;
    }
    await api.notified.add({ issue_id: issue.id, title: issue.title });
    notified.push(issue.id);
  }
  return { pages, candidates: candidates.length, already, notified, failed };
}
