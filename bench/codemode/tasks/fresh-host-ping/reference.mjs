async function ping(api, host) {
  try {
    return await api.hosts.ping({ host });
  } catch (error) {
    if (error.kind !== "Timeout") return null;
  }
  await api.sleep(3000);
  try {
    return await api.hosts.ping({ host });
  } catch {
    return null;
  }
}

export default async function main(api) {
  const up = [];
  const down = [];
  for (const host of ["h1", "h2", "h3", "h4", "h5"]) {
    const pong = await ping(api, host);
    if (pong === null) down.push(host);
    else up.push(pong);
  }
  up.sort((a, b) => a.ms - b.ms || (a.host < b.host ? -1 : 1));
  return { up: up.map((p) => p.host), down };
}
