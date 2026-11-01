export default async function main(api) {
  const first = await api.tickets.list({ status: "open", page: 1 });
  const tickets = [...first.items];
  for (let page = 2; page <= first.pages; page += 1) {
    tickets.push(...(await api.tickets.list({ status: "open", page })).items);
  }
  const out = [];
  for (const priority of ["P1", "P2", "P3"]) {
    const ids = tickets
      .filter((t) => (t.priority ?? "P3") === priority)
      .map((t) => t.id)
      .sort((a, b) => a - b);
    if (ids.length > 0) out.push({ priority, ids });
  }
  return out;
}
