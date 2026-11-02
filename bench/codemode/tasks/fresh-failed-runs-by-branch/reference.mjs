export default async function main(api) {
  const counts = {};
  for (let page = 1; page <= 15; page++) {
    const result = await api.runs.page({ page });
    for (const run of result.runs) {
      if (run.status === "failed") counts[run.branch] = (counts[run.branch] ?? 0) + 1;
    }
    if (result.last) return counts;
  }
  throw new Error("more than 15 pages");
}
