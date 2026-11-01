export default async function main(api) {
  const waiting = [];
  let scanned = 0;
  let after = null;
  do {
    const page = await api.github.listPrs({ repo: "laputa/xsh", after });
    for (const pr of page.items) {
      scanned += 1;
      if (pr.state === "open" && !pr.draft && pr.reviewers.length === 0) waiting.push(pr.number);
    }
    after = page.next;
  } while (after !== null);
  return { waiting: waiting.sort((a, b) => a - b), scanned };
}
