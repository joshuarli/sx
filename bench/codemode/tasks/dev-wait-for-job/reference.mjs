export default async function main(api) {
  const job = await api.jobs.start({ name: "reindex" });
  for (let waited = 0; ; waited += 5000) {
    const status = await api.jobs.status({ id: job.id });
    if (status.state === "done" || status.state === "failed") return { state: status.state, detail: status.detail };
    if (waited + 5000 > 120000) throw new Error("job did not finish within 2 minutes");
    await api.sleep(5000);
  }
}
