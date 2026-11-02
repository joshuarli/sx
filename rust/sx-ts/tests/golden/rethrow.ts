async function readOrDefault(path) {
  try {
    return await tools.fs.readText({ path });
  } catch (e) {
    if (e.kind === "NotFound") return "";
    throw e;
  }
}
const [a, b] = await Promise.all([readOrDefault("a"), readOrDefault("b")]);
return a + b;
