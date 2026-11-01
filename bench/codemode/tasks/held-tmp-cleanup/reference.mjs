export default async function main(api) {
  const files = (await api.files.list({ dir: "scratch" }))
    .filter((f) => api.text.endsWith(f.name, ".tmp") && f.modified <= 193)
    .sort((a, b) => (a.name < b.name ? -1 : 1));
  const deleted = [];
  const failed = [];
  let freed = 0;
  for (const file of files) {
    try {
      await api.files.delete({ path: "scratch/" + file.name });
      deleted.push(file.name);
      freed += file.bytes;
    } catch (error) {
      if (error.kind !== "NotFound") failed.push(file.name);
    }
  }
  return { deleted, freed, failed };
}
