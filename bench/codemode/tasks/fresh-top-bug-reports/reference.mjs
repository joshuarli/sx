export default async function main(api) {
  const open = [];
  let cursor = null;
  for (let page = 0; page < 10; page++) {
    const result = await api.issues.search({ repo: "acme/api", label: "bug", cursor });
    open.push(...result.items.filter((i) => i.assignee === null));
    if (result.next === null) break;
    cursor = result.next;
  }
  open.sort((a, b) => b.comments - a.comments || a.id - b.id);
  return open.slice(0, 3).map((i) => i.id);
}
