const prefix = "p-";
function double(n) { return n * 2; }
const triple = (n) => n * 3;
function label(n) { return prefix + n; }
async function load(path) { return await tools.fs.readText({ path }); }
const t = await load("a");
return { d: double(2), t3: triple(3), l: label(1), t };
