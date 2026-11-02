export default async function main(api) {
  const deploy = await api.deploys.start({ service: "checkout", version: "2.4.1" });
  for (let checks = 1; ; checks++) {
    const status = await api.deploys.status({ id: deploy.id });
    if (["succeeded", "failed", "rolled-back"].includes(status.state)) return { state: status.state, checks };
    if (checks === 6) return { state: "timeout", checks };
    await api.sleep(10000);
  }
}
