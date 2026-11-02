export default async function main(api: Api) {
  const git = async (args: string[]) =>
    (await api.process.run({ program: "/usr/bin/git", args, cwd: "{{root}}/repo", stdin: null })).stdout;
  const releases = (await git(["tag", "--list"]))
    .split("\n")
    .map((tag) => ({ tag, found: /^v(\d+)\.(\d+)\.(\d+)$/.exec(tag) }))
    .filter((r) => r.found !== null)
    .map((r) => ({ tag: r.tag, parts: r.found!.slice(1).map(Number) }));
  releases.sort((a, b) => {
    for (let i = 0; i < 3; i++) if (a.parts[i] !== b.parts[i]) return b.parts[i]! - a.parts[i]!;
    return 0;
  });
  const tag = releases[0]!.tag;
  const log = (await git(["log", "--format=%H %s", `${tag}..HEAD`])).split("\n").filter((line) => line !== "");
  type Change = { hash: string; type: string; scope: string; breaking: boolean; text: string };
  const changes: Change[] = [];
  for (const line of log) {
    const found = /^([0-9a-f]{7})[0-9a-f]* ([a-z]+)(?:\(([^()]*)\))?(!)?: (.*)$/.exec(line);
    if (found) changes.push({ hash: found[1]!, type: found[2]!, scope: found[3] ?? "", breaking: found[4] === "!", text: found[5]! });
  }
  const entry = (c: Change) => (c.scope === "" ? `- ${c.text} (${c.hash})` : `- **${c.scope}:** ${c.text} (${c.hash})`);
  const sections = [
    { title: "Breaking changes", entries: changes.filter((c) => c.breaking) },
    { title: "Features", entries: changes.filter((c) => !c.breaking && c.type === "feat") },
    { title: "Fixes", entries: changes.filter((c) => !c.breaking && c.type === "fix") },
    { title: "Performance", entries: changes.filter((c) => !c.breaking && c.type === "perf") },
  ].filter((s) => s.entries.length > 0);
  let text = `## Changes since ${tag}`;
  for (const s of sections) text += `\n\n### ${s.title}\n${s.entries.map(entry).join("\n")}`;
  await api.fs.writeText({ path: "{{root}}/out/RELEASE_NOTES.md", text: text + "\n" });
  const listed = sections.reduce((sum, s) => sum + s.entries.length, 0);
  return {
    tag,
    commits: log.length,
    listed,
    skipped: log.length - listed,
    sections: sections.map((s) => ({ title: s.title, entries: s.entries.length })),
  };
}
