type Observed = { repo: string; clean: boolean; readable: boolean; sha: string; branch: string; ahead: number };

export default async function main(api: Api) {
  const listing = await api.fs.readText({ path: "{{root}}/repos.txt" });
  const names = listing.split("\n").map((line) => line.trim()).filter((line) => line !== "");
  const git = async (dir: string, args: string[]) =>
    api.process.run({ program: "/usr/bin/git", args, cwd: dir, stdin: null });
  const observe = async (repo: string): Promise<Observed> => {
    const dir = `{{root}}/repos/${repo}`;
    const none = { repo, clean: false, readable: false, sha: "", branch: "", ahead: 0 };
    let status;
    try {
      status = await git(dir, ["status", "--porcelain"]);
    } catch {
      return none;
    }
    if (status.status !== 0) return none;
    if (status.stdout !== "") return { ...none, readable: true };
    const sha = (await git(dir, ["rev-parse", "HEAD"])).stdout.trim();
    const branch = (await git(dir, ["branch", "--show-current"])).stdout.trim();
    const ahead = Number((await git(dir, ["rev-list", "--count", "main..HEAD"])).stdout.trim());
    return { repo, clean: true, readable: true, sha, branch, ahead };
  };
  const observed = await Promise.all(names.map(observe));
  const clean = observed.filter((o) => o.clean);
  const checked: { o: Observed; state: string }[] = [];
  for (let at = 0; at < clean.length; at += 8) {
    const batch = await Promise.all(
      clean.slice(at, at + 8).map(async (o) => {
        try {
          const found = await api.ci.checks({ repo: o.repo, sha: o.sha });
          return { o, state: found.state };
        } catch {
          return { o, state: "unknown" };
        }
      }),
    );
    checked.push(...batch);
  }
  const failing = checked
    .filter((c) => c.state === "failure")
    .map((c) => ({ repo: c.o.repo, branch: c.o.branch, ahead: c.o.ahead }))
    .sort((a, b) => b.ahead - a.ahead || (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0));
  const unreadable = observed.filter((o) => !o.readable).map((o) => o.repo).sort();
  return { failing, unreadable, unknown: checked.filter((c) => c.state === "unknown").length };
}
