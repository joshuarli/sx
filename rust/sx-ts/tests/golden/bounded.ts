const t = await retry({ delaysMs: [1000, 2000], on: ["Timeout", "Unavailable"] }, () => tools.fs.readText({ path: "a" }));
const u = await retry({ backoffMs: [100, 5000], withinMs: 30000 }, async () => await tools.fs.readText({ path: "b" }));
const job = await waitUntil({ everyMs: 15000, withinMs: 600000 }, () => tools.jobs.status({ id: "j" }), (v) => v.state === "done");
await sleep(2000);
const sizes = await parMap({ limit: 8 }, ["a", "b"], (p) => tools.fs.readText({ path: p }));
const all = await loop({ max: 50 }, { cursor: null, events: [] }, async ({ cursor, events }) => {
  const page = await tools.mcpGithub.listIssues({ repo: "r", cursor });
  const acc = events.concat(page.items);
  if (page.next != null) return next({ cursor: page.next, events: acc });
  return done(acc);
});
return { t, u, job, sizes, all };
