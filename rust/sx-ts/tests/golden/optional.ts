const page = await tools.mcpGithub.listIssues({ repo: "r" });
const n = page.next ?? "none";
const m = page.next?.length ?? 0;
const label = page.next != null ? page.next : "end";
if (page.next) { return page.next; }
if (page.next == null) { return "x"; }
return [n, m, label];
