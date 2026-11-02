export default async function main(api: Api) {
  const dir = "{{root}}/data";
  const entries = await api.fs.list({ path: `${dir}` });
  const files = entries.filter((entry) => entry.kind === "File");
  const stats = await Promise.all(files.map((entry) => api.fs.stat({ path: `${dir}/${entry.name}` })));
  const bytes = stats.reduce((sum, stat) => sum + (stat === null ? 0 : stat.size), 0);
  await api.fs.writeText({ path: "{{root}}/summary.txt", text: `files: ${files.length}\nbytes: ${bytes}\n` });
  return { count: files.length, bytes };
}
