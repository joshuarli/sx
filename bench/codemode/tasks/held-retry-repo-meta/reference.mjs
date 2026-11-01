async function fetchWithRetry(api, id) {
  const waits = [2000, 4000, 8000];
  for (let attempt = 1; ; attempt += 1) {
    try {
      const meta = await api.github.getRepo({ id });
      return { repo: id, attempts: attempt, stars: meta.stars };
    } catch (error) {
      const retryable = error.kind === "RateLimited" || error.kind === "Timeout";
      if (!retryable || attempt > waits.length) return { repo: id, attempts: attempt, stars: null };
      await api.sleep(waits[attempt - 1]);
    }
  }
}

export default async function main(api) {
  const out = [];
  for (const id of ["laputa/xsh", "laputa/diff-pretty", "laputa/old"]) out.push(await fetchWithRetry(api, id));
  return out;
}
