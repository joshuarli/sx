export default async function main(api) {
  const issues = await api.issues.open({ repo: "acme/web" });
  const labeled = [];
  const failed = [];
  for (const issue of issues.filter((i) => i.labels.length === 0)) {
    try {
      await api.issues.addLabel({ repo: "acme/web", id: issue.id, label: "needs-triage" });
      labeled.push(issue.id);
    } catch (error) {
      failed.push({ id: issue.id, kind: error.kind });
    }
  }
  return { labeled, failed };
}
