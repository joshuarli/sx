export default async function main(api: Api) {
  const git = async (args: string[]) =>
    (await api.process.run({ program: "/usr/bin/git", args, cwd: "{{root}}/repo", stdin: null })).stdout;
  const releases = (await git(["tag", "--list"]))
    .split("\n")
    .map((tag) => ({ tag, found: /^v(\d+)\.(\d+)\.(\d+)$/.exec(tag) }))
    .filter((entry) => entry.found !== null)
    .map((entry) => ({ tag: entry.tag, numbers: entry.found!.slice(1).map(Number) }));
  const newer = (a: number[], b: number[]) => a[0]! - b[0]! || a[1]! - b[1]! || a[2]! - b[2]!;
  const latest = releases.reduce((best, c) => (newer(c.numbers, best.numbers) > 0 ? c : best));
  const subjects = (await git(["log", "--format=%s", `${latest.tag}..HEAD`])).trim().split("\n");
  const commits: { type: string; breaking: boolean; description: string }[] = [];
  for (const subject of subjects) {
    const found = /^(feat|fix|perf|docs|chore|refactor|test|build|ci)(?:\([^)]*\))?(!)?: (.+)$/.exec(subject);
    if (found) commits.push({ type: found[1]!, breaking: found[2] === "!", description: found[3]! });
  }
  const [major, minor, patch] = latest.numbers as [number, number, number];
  let bump: "major" | "minor" | "patch" = "patch";
  if (commits.some((c) => c.breaking)) bump = "major";
  else if (commits.some((c) => c.type === "feat")) bump = "minor";
  const version =
    bump === "major" ? `${major + 1}.0.0` : bump === "minor" ? `${major}.${minor + 1}.0` : `${major}.${minor}.${patch + 1}`;
  const tag = `v${version}`;
  const notes = commits
    .filter((c) => c.type === "feat" || c.type === "fix" || c.type === "perf")
    .map((c) => `- ${c.description}`)
    .join("\n");
  await git(["tag", tag]);
  const commit = (await git(["rev-parse", "HEAD"])).trim();
  let published: { id: number };
  for (let attempt = 0; ; attempt++) {
    try {
      published = await api.releases.create({ tag, notes });
      break;
    } catch (error) {
      if ((error as CallError).kind !== "RateLimited" || attempt === 2) throw error;
      await api.sleep(100);
    }
  }
  return { latest: latest.tag, since: subjects.length, bump, tag, commit, release_id: published.id, notes };
}
