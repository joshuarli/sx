export default async function main(api) {
  const services = ["api", "web", "worker"];
  const outcomes = await Promise.all(services.map((service) => api.deploy.last({ service })));
  return outcomes.map((outcome, i) => {
    const name = services[i];
    if (outcome === "Skipped") return `${name}: skipped`;
    if (outcome.tag === "Succeeded") return `${name}: ok ${outcome.value.url}`;
    return `${name}: failed ${outcome.value.reason}`;
  });
}
