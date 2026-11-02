let t;
try {
  t = await tools.fs.readText({ path: "a" });
} catch {
  t = "";
}
try {
  await tools.fs.writeText({ path: "b", text: t });
} catch (e) {
  console.log("failed", e.message);
  throw new Error("write failed: " + e.message);
}
return t;
