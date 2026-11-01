const byRepo = (a, b) => (a.repo < b.repo ? -1 : a.repo > b.repo ? 1 : 0);

export default async function main(api) {
  const checkouts = await api.checkouts.list();
  const pulled = [];
  const skipped = [];
  for (const checkout of checkouts) {
    const status = await api.git.status({ path: checkout.path });
    if (status.behind === 0) continue;
    if (!status.dirty && status.ahead === 0) {
      const { commits } = await api.git.pull({ path: checkout.path });
      pulled.push({ repo: checkout.repo, commits });
    } else {
      skipped.push({ repo: checkout.repo, reason: status.dirty ? "dirty" : "diverged" });
    }
  }
  return { pulled: pulled.sort(byRepo), skipped: skipped.sort(byRepo) };
}
