type Row = { id: string; name: string; email: string; team: string | null };

export default async function main(api: Api) {
  const ids: string[] = [];
  for (let offset = 0; ; offset += 20) {
    const page = await api.people.list({ offset, limit: 20 });
    ids.push(...page.people.map((p) => p.id));
    if (offset + 20 >= page.total) break;
  }
  const delays = [50, 150];
  const fetchOne = async (id: string) => {
    for (let attempt = 0; ; attempt++) {
      try {
        const p = await api.people.get({ id });
        return { id, state: "found" as const, row: { id, name: p.name, email: p.email.trim().toLowerCase(), team: p.team } };
      } catch (error) {
        const kind = (error as CallError).kind;
        if (kind === "NotFound") return { id, state: "gone" as const, row: null };
        if (kind !== "RateLimited") return { id, state: "failed" as const, row: null };
        if (attempt === delays.length) return { id, state: "failed" as const, row: null };
        await api.sleep(delays[attempt]!);
      }
    }
  };
  const fetched: Awaited<ReturnType<typeof fetchOne>>[] = [];
  let next = 0;
  const worker = async () => {
    while (next < ids.length) {
      const id = ids[next++]!;
      fetched.push(await fetchOne(id));
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  const rows = await api.people.all({});
  const known = new Map<string, Row>(rows.map((r) => [r.id, r]));
  const inserted: string[] = [];
  const updated: string[] = [];
  let unchanged = 0;
  for (const f of [...fetched].sort((a, b) => (a.id < b.id ? -1 : 1))) {
    if (f.row === null) continue;
    const old = known.get(f.id);
    if (old === undefined) {
      await api.people.insert(f.row);
      inserted.push(f.id);
    } else if (old.name !== f.row.name || old.email !== f.row.email || old.team !== f.row.team) {
      await api.people.update(f.row);
      updated.push(f.id);
    } else {
      unchanged++;
    }
  }
  const deleted = rows.map((r) => r.id).filter((id) => !ids.includes(id)).sort();
  for (const id of deleted) await api.people.delete({ id });
  const withState = (state: string) => fetched.filter((f) => f.state === state).map((f) => f.id).sort();
  return {
    listed: ids.length,
    inserted: inserted.sort(),
    updated: updated.sort(),
    deleted,
    unchanged,
    gone: withState("gone"),
    failed: withState("failed"),
  };
}
