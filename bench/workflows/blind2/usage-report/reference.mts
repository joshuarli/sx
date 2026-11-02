type Account = { id: string; name: string; plan: string };
type Measured = Account & { state: "ok" | "closed" | "unreachable"; requests: number; errors: number };

export default async function main(api: Api) {
  const delays = [30, 60];
  const refused = (error: unknown) => {
    const kind = (error as CallError).kind;
    return kind === "RateLimited" || kind === "Unavailable";
  };
  const withRepeats = async <T,>(request: () => Promise<T>): Promise<T> => {
    for (let attempt = 0; ; attempt++) {
      try {
        return await request();
      } catch (error) {
        if (!refused(error) || attempt === delays.length) throw error;
        await api.sleep(delays[attempt]!);
      }
    }
  };
  const listed: Account[] = [];
  for (let offset = 0; ; offset += 20) {
    const page = await withRepeats(() => api.accounts.page({ limit: 20, offset }));
    listed.push(...page.items);
    if (offset + 20 >= page.total) break;
  }
  const measured: Measured[] = new Array(listed.length);
  let next = 0;
  const worker = async () => {
    while (next < listed.length) {
      const index = next++;
      const account = listed[index]!;
      try {
        const usage = await withRepeats(() => api.accounts.usage({ id: account.id }));
        measured[index] = { ...account, state: "ok", requests: usage.requests, errors: usage.errors };
      } catch (error) {
        const state = (error as CallError).kind === "NotFound" ? "closed" : "unreachable";
        measured[index] = { ...account, state, requests: 0, errors: 0 };
      }
    }
  };
  await Promise.all([worker(), worker(), worker(), worker()]);
  const ids = (state: string) => measured.filter((m) => m.state === state).map((m) => m.id).sort();
  const ranked = measured
    .filter((m) => m.state === "ok")
    .map((m) => ({ ...m, rate: m.requests === 0 ? 0 : Math.floor((m.errors * 1000) / m.requests) }))
    .sort((a, b) => b.rate - a.rate || b.requests - a.requests || (a.id < b.id ? -1 : a.id > b.id ? 1 : 0));
  const top = ranked.slice(0, 10);
  const cell = (name: string) => (/[",]/.test(name) ? `"${name.replaceAll('"', '""')}"` : name);
  const lines = ["id,name,plan,requests,errors,rate"];
  for (const m of top) lines.push([m.id, cell(m.name), m.plan, m.requests, m.errors, m.rate].join(","));
  await api.fs.writeText({ path: "{{root}}/usage.csv", text: lines.join("\n") + "\n" });
  return {
    listed: listed.length,
    closed: ids("closed"),
    unreachable: ids("unreachable"),
    requests: ranked.reduce((sum, m) => sum + m.requests, 0),
    top: top.map((m) => ({ id: m.id, rate: m.rate })),
  };
}
