type Job = { id: string; state: "ready" | "failed" | "timeout"; parts: number; bytes: number };

export default async function main(api: Api) {
  const create = async (report: string) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return (await api.export.create({ report })).id;
      } catch (error) {
        if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
        await api.sleep(100);
      }
    }
  };
  const runJob = async (report: string): Promise<Job> => {
    const id = await create(report);
    let waited = 0;
    for (let delay = 50; ; delay = Math.min(delay * 2, 200)) {
      const s = await api.export.status({ id });
      if (s.status === "ready") return { id, state: "ready", parts: s.parts ?? 0, bytes: s.bytes ?? 0 };
      if (s.status === "failed") return { id, state: "failed", parts: 0, bytes: 0 };
      if (waited + delay > 1500) return { id, state: "timeout", parts: 0, bytes: 0 };
      await api.sleep(delay);
      waited += delay;
    }
  };
  const part = async (id: string, n: number) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return (await api.export.part({ id, n })).text;
      } catch (error) {
        if ((error as CallError).kind !== "Unavailable" || attempt === 2) throw error;
        await api.sleep(50);
      }
    }
  };
  const reports = [];
  for (const report of ["summary", "detail", "audit", "ledger"]) {
    let job = await runJob(report);
    let jobs = 1;
    if (job.state === "failed") {
      job = await runJob(report);
      jobs = 2;
    }
    if (job.state !== "ready") {
      reports.push({ report, outcome: job.state, jobs, parts: 0, bytes: 0 });
      continue;
    }
    const texts: string[] = new Array(job.parts);
    let next = 1;
    const worker = async () => {
      while (next <= job.parts) {
        const n = next++;
        texts[n - 1] = await part(job.id, n);
      }
    };
    await Promise.all([worker(), worker()]);
    const body = texts.join("");
    if (body.length === job.bytes) {
      await api.fs.writeText({ path: `{{root}}/exports/${report}.csv`, text: body });
      reports.push({ report, outcome: "written", jobs, parts: job.parts, bytes: body.length });
    } else {
      reports.push({ report, outcome: "corrupt", jobs, parts: job.parts, bytes: 0 });
    }
  }
  return { reports, written: reports.filter((r) => r.outcome === "written").length };
}
