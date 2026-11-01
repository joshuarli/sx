export default async function main(api) {
  const repo = "laputa/xsh";
  const prs = (await api.github.listOpenPrs({ repo })).filter((pr) => !pr.draft);
  const states = await Promise.all(prs.map((pr) => api.ci.prState({ repo, number: pr.number })));
  const ready = prs.filter((pr, i) => states[i] === "Passing").map((pr) => pr.number).sort((a, b) => a - b);
  const merged = [];
  const rejected = [];
  for (const number of ready) {
    try {
      await api.github.mergePr({ repo, number });
      merged.push(number);
    } catch (error) {
      rejected.push({ number, kind: error.kind });
    }
  }
  return { merged, rejected };
}
