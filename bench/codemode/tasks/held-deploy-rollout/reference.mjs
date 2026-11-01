export default async function main(api) {
  const { deployment } = await api.deploy.start({ service: "checkout", version: "2.4.1" });
  for (let checks = 1; checks <= 8; checks += 1) {
    await api.sleep(5000);
    const state = await api.deploy.status({ deployment });
    if (state === "Healthy") return { outcome: "healthy", checks, detail: null, restored: null };
    if (typeof state === "object" && state.tag === "Failed") {
      const { restored } = await api.deploy.rollback({ deployment });
      return { outcome: "failed", checks, detail: state.value, restored };
    }
  }
  return { outcome: "timeout", checks: 8, detail: null, restored: null };
}
