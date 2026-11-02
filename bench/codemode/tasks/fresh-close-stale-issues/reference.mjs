export default async function main(api) {
  const issues = await api.issues.list({ repo: "acme/docs" });
  const closed = [];
  const skipped = [];
  for (const issue of issues.filter((i) => i.idle > 90 && i.reactions === 0)) {
    try {
      await api.issues.comment({ repo: "acme/docs", id: issue.id, body: "Closing due to inactivity." });
      await api.issues.close({ repo: "acme/docs", id: issue.id });
      closed.push(issue.id);
    } catch {
      skipped.push(issue.id);
    }
  }
  return { closed, skipped };
}
