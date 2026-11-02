type Outcome = "exported" | "timeout" | "failed";

export default async function main(api: Api) {
  const create = async (report: string) => {
    for (let attempt = 0; ; attempt++) {
      try {
        return (await api.exports.create({ report, month: "2026-03" })).id;
      } catch (error) {
        if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
        await api.sleep(50);
      }
    }
  };
  const watch = async (id: string) => {
    let waited = 0;
    let wait = 50;
    for (;;) {
      const status = await api.exports.status({ id });
      if (status.state !== "queued" && status.state !== "running") return status;
      if (waited + wait > 1000) return null;
      await api.sleep(wait);
      waited += wait;
      wait = Math.min(wait * 2, 200);
    }
  };
  const reports: { report: string; outcome: Outcome; jobs: string[]; rows: number | null }[] = [];
  for (const report of ["sales", "refunds", "payouts", "adjustments"]) {
    const jobs: string[] = [];
    let outcome: Outcome = "failed";
    let rows: number | null = null;
    for (;;) {
      const id = await create(report);
      jobs.push(id);
      const status = await watch(id);
      if (status === null) {
        outcome = "timeout";
        break;
      }
      if (status.state === "done") {
        const { lines } = await api.exports.download({ id });
        await api.fs.writeText({ path: `{{root}}/exports/${report}.csv`, text: lines.map((l) => `${l}\n`).join("") });
        outcome = "exported";
        rows = lines.length;
        break;
      }
      if (status.reason !== "transient" || jobs.length === 3) break;
    }
    reports.push({ report, outcome, jobs, rows });
  }
  return { reports };
}
