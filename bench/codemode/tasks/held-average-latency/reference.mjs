export default async function main(api) {
  const services = (await api.metrics.services()).sort();
  const samples = await Promise.all(services.map((service) => api.metrics.latency({ service })));
  const means = services.map((service, i) => ({
    service,
    mean: samples[i].length === 0 ? null : Math.round((samples[i].reduce((a, b) => a + b, 0) / samples[i].length) * 100) / 100,
  }));
  let slowest = null;
  for (const m of means) {
    if (m.mean !== null && (slowest === null || m.mean > slowest.mean)) slowest = m;
  }
  return { means, slowest: slowest ? slowest.service : null };
}
