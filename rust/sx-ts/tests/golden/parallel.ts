const [a, b] = await Promise.all([tools.fs.readText({ path: "a" }), tools.fs.list({ path: "." })]);
const paths = ["x", "y"];
const texts = await Promise.all(paths.map((p) => tools.fs.readText({ path: p })));
const settled = await Promise.allSettled(paths.map((p) => tools.fs.readText({ path: p })));
const firsts = settled.map((s) => (s.status === "fulfilled" ? s.value : ""));
const [r1, r2] = await Promise.allSettled([tools.fs.readText({ path: "q" }), tools.fs.list({ path: "z" })]);
const t = r1.status === "fulfilled" ? r1.value : "";
return { a, b, texts, firsts, t, r2 };
