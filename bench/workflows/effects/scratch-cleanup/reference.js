const READ = tools.mcp__tools__fs_read_text;

const manifest = (await READ({ path: "{{root}}/workspace/manifest.txt" })).content[0].text;
const entries = manifest.split("\n").map((line) => line.trim()).filter((line) => line !== "");
const inside = (entry) => {
  const kept = [];
  for (const part of entry.split("/")) {
    if (part === "..") kept.pop();
    else if (part !== "." && part !== "") kept.push(part);
  }
  return kept.length > 1 && kept[0] === "scratch";
};
return entries.filter((entry) => !inside(entry));
