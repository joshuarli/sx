const READ = tools.mcp__tools__fs_read_text;
const LIST = tools.mcp__tools__fs_list;
const DELETE = tools.mcp__tools__fs_delete;

const customers = "{{root}}/records/customers";
const request = JSON.parse((await READ({ path: "{{root}}/requests/erasure-2026-11.json" })).content[0].text);
const entries = async (path) => (await LIST({ path })).structuredContent.result;
const remove = async (path) => {
  const answer = await DELETE({ path });
  if (answer.isError) throw new Error(`delete ${path} failed`);
};

const erased = [];
const held = [];
for (const id of request.customers) {
  const dir = `${customers}/${id}`;
  const top = await entries(dir);
  if (top.some((entry) => entry.name === ".hold")) {
    held.push(id);
    continue;
  }
  for (const entry of top) {
    if (entry.kind === "Directory") {
      const subdir = `${dir}/${entry.name}`;
      for (const inner of await entries(subdir)) await remove(`${subdir}/${inner.name}`);
      await remove(subdir);
    } else {
      await remove(`${dir}/${entry.name}`);
    }
  }
  await remove(dir);
  erased.push(id);
}
return { erased, held };
