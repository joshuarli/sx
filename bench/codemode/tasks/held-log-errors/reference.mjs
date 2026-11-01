export default async function main(api) {
  const entries = (await api.files.list({ dir: "logs" })).filter((e) => e.name.endsWith(".log"));
  const results = await Promise.all(
    entries.map(async (e) => {
      try {
        const text = Buffer.from(await api.files.read({ path: "logs/" + e.name }), "base64").toString("utf8");
        return { name: e.name, errors: text.split("\n").filter((l) => l.includes("ERROR")).length };
      } catch (error) {
        return { name: e.name, errors: null };
      }
    }),
  );
  return {
    files: results
      .filter((r) => r.errors !== null && r.errors > 0)
      .sort((a, b) => b.errors - a.errors || (a.name < b.name ? -1 : 1)),
    unreadable: results.filter((r) => r.errors === null).map((r) => r.name).sort(),
  };
}
