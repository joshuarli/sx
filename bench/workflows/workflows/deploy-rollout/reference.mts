type Outcome = "done" | "rolled-back" | "timeout";

export default async function main(api: Api) {
  const table: [string, string][] = [
    ["api", "1.4.0"],
    ["web", "2.1.0"],
    ["worker", "0.9.3"],
    ["billing", "3.0.0"],
    ["search", "1.1.1"],
  ];
  const start = async (service: string, version: string) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return (await api.deploy.start({ service, version })).id;
      } catch (error) {
        if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
        await api.sleep(100);
      }
    }
  };
  const started: { service: string; id: string }[] = [];
  for (const [service, version] of table) started.push({ service, id: await start(service, version) });
  const watch = async (id: string): Promise<"done" | "failed" | "timeout"> => {
    const first = Date.now();
    for (;;) {
      const { status } = await api.deploy.status({ id });
      if (status === "done" || status === "failed") return status;
      if (Date.now() - first >= 1000) return "timeout";
      await api.sleep(100);
    }
  };
  const watched = await Promise.all(started.map(async (d) => ({ ...d, status: await watch(d.id) })));
  const deployments: { service: string; id: string; outcome: Outcome }[] = [];
  for (const { service, id, status } of watched) {
    if (status === "failed") await api.deploy.rollback({ id });
    deployments.push({ service, id, outcome: status === "failed" ? "rolled-back" : status });
  }
  const count = (outcome: Outcome) => deployments.filter((d) => d.outcome === outcome).length;
  return { deployments, done: count("done"), rolled_back: count("rolled-back"), timed_out: count("timeout") };
}
