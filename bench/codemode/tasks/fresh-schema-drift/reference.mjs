const byName = (a, b) => (a < b ? -1 : 1);

export default async function main(api) {
  const staging = new Map((await api.tables.columns({ env: "staging", table: "events" })).map((c) => [c.name, c.type]));
  const prod = new Map((await api.tables.columns({ env: "prod", table: "events" })).map((c) => [c.name, c.type]));
  const added = [...staging.keys()].filter((n) => !prod.has(n)).sort(byName);
  const removed = [...prod.keys()].filter((n) => !staging.has(n)).sort(byName);
  const changed = [...staging.keys()]
    .filter((n) => prod.has(n) && prod.get(n) !== staging.get(n))
    .sort(byName)
    .map((column) => ({ column, from: prod.get(column), to: staging.get(column) }));
  return { added, removed, changed };
}
