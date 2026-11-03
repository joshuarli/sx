const settled = await Promise.allSettled(["x", "y"].map((p) => tools.fs.readText({ path: p })));
let good = 0;
for (const s of settled) {
  if (s.status === "fulfilled") {
    good += s.value.length;
  }
}
return good;
