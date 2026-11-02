const config = { name: "x", counts: { a: 1 } };
config.name = "y";
config.counts.a += 2;
const index = {};
for (const f of await tools.fs.list({ path: "." })) {
  index[f.name] = f.size;
}
const rows = [3, 1, 2].map((n) => n);
rows.sort((a, b) => a - b);
rows.reverse();
let label = "n:";
label += rows.length;
return { config, index, rows, label };
