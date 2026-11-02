export default async function main(api) {
  const summaries = [];
  for (const service of ["auth", "search", "cart"]) {
    const samples = await api.metrics.latency({ service });
    if (samples.length === 0) continue;
    summaries.push({ service, mean: samples.reduce((a, b) => a + b, 0) / samples.length, max: Math.max(...samples) });
  }
  return summaries.sort((a, b) => b.mean - a.mean || (a.service < b.service ? -1 : 1));
}
