const files = await tools.fs.list({ path: "." });
let found = "";
for (const f of files) {
  if (f.size === 0) continue;
  if (f.name === "stop") break;
  if (f.size > 1000) return f.name;
  found = f.name;
}
return found;
