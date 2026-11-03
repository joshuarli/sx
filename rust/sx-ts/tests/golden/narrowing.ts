const page = await tools.mcpGithub.listIssues({ repo: "r" });
const next = page.next;
const out = [];
for (const issue of page.items) {
  const text = page.next;
  if (text == null) { continue; }
  out.push(text.length);
}
const a = next != null && next.length > 0;
const b = next == null || next.length > 3;
const c = next == null ? 0 : next.length;
const d = next ? next.length : 0;
if (next == null) { throw new Error("no next"); }
const e = next != null ? 1 : 0;
return [out, a, b, c, d, e, next.length];
