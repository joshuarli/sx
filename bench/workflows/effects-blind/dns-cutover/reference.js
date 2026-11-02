const READ = tools.mcp__tools__fs_read_text;
const CREATE = tools.mcp__tools__dns_create;
const REMOVE = tools.mcp__tools__dns_remove;

const changes = JSON.parse((await READ({ path: "{{root}}/changes/cutover.json" })).content[0].text).records;
const created = [];
for (const record of changes) {
  const answer = await CREATE(record);
  if (answer.isError) {
    for (const id of created) {
      const undone = await REMOVE({ id });
      if (undone.isError) throw new Error(`could not delete ${id}`);
    }
    return { applied: false, refused: record.name };
  }
  created.push(answer.structuredContent.id);
}
return { applied: true, refused: "" };
