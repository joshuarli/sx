const TRANSIENT = new Set(["Timeout", "Unavailable", "RateLimited"]);

export default async function main(api) {
  const results = [];
  for (const repo of ["acme/api", "acme/old", "acme/web"]) {
    results.push({ repo, state: await stateWithRetry(api, repo) });
  }
  return results;
}

async function stateWithRetry(api, repo) {
  const delays = [1000, 2000, 4000];
  for (let attempt = 0; ; attempt++) {
    try {
      return (await api.github.latestCi({ repo })).state;
    } catch (error) {
      if (!TRANSIENT.has(error.kind) || attempt === delays.length) return "missing";
      await api.sleep(delays[attempt]);
    }
  }
}
