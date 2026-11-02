type Stage = { percent: number; result: "passed" | "degraded" | "timeout"; probes: number };

export default async function main(api: Api) {
  const setWeight = async (name: string, percent: number) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return await api.canary.setWeight({ name, percent });
      } catch (error) {
        if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
        await api.sleep(100);
      }
    }
  };
  const probe = async (name: string): Promise<{ result: Stage["result"]; probes: number }> => {
    let good = 0;
    let bad = 0;
    for (let n = 1; ; n++) {
      const { state } = await api.canary.health({ name });
      good = state === "healthy" ? good + 1 : 0;
      bad = state === "degraded" ? bad + 1 : 0;
      if (good >= 3) return { result: "passed", probes: n };
      if (bad >= 2) return { result: "degraded", probes: n };
      if (n === 12) return { result: "timeout", probes: n };
      await api.sleep(50);
    }
  };
  const services: { service: string; outcome: string; stages: Stage[]; final_weight: number }[] = [];
  for (const name of ["checkout", "search", "billing"]) {
    const stages: Stage[] = [];
    for (const percent of [5, 25, 50, 100]) {
      await setWeight(name, percent);
      const { result, probes } = await probe(name);
      stages.push({ percent, result, probes });
      if (result !== "passed") break;
    }
    const promoted = stages[stages.length - 1]!.result === "passed";
    if (!promoted) await setWeight(name, 0);
    services.push({ service: name, outcome: promoted ? "promoted" : "rolled-back", stages, final_weight: promoted ? 100 : 0 });
  }
  const count = (outcome: string) => services.filter((s) => s.outcome === outcome).length;
  return { services, promoted: count("promoted"), rolled_back: count("rolled-back") };
}
