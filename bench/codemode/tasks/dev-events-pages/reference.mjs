export default async function main(api) {
  const ids = [];
  let cursor = null;
  for (let page = 0; page < 20; page++) {
    const result = await api.audit.page({ cursor });
    ids.push(...result.events.filter((e) => e.kind === "error").map((e) => e.id));
    if (result.next === null) return ids;
    cursor = result.next;
  }
  throw new Error("audit log has more than 20 pages");
}
