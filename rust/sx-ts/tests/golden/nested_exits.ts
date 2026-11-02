const files = await tools.fs.list({ path: "." });
for (const f of files) {
  for (const g of files) {
    if (g.size > f.size) continue;
    if (g.name === "stop") return g.name;
  }
}
try {
  for (const f of files) {
    if (f.size < 0) break;
    await tools.fs.readText({ path: f.name });
  }
} catch (e) {
  return String(e);
}
return null;
