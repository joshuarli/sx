try {
  const r = await tools.fs.readText({ path: "a" });
  return "ok " + r;
} catch (e) {
  return "failed " + e.message;
}
