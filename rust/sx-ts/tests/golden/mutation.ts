let total = 0;
const names = [];
for (const f of await tools.fs.list({ path: "." })) {
  total += f.size;
  names.push(f.name);
}
return { total, names };
