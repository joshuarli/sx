const text = await tools.fs.readText({ path: "a.txt" });
const files = await tools.fs.list({ path: "." });
const big = files.filter((f) => f.size > 100).map((f) => f.name);
return { text, big, n: big.length, label: `n=${big.length}` };
