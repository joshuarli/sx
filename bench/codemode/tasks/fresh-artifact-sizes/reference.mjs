const DELAYS = [500, 1500];

async function size(api, build) {
  for (let attempt = 0; ; attempt++) {
    try {
      return await api.artifacts.size({ build });
    } catch (error) {
      if (!["Timeout", "Unavailable"].includes(error.kind) || attempt === DELAYS.length) return null;
      await api.sleep(DELAYS[attempt]);
    }
  }
}

export default async function main(api) {
  let total = 0;
  const missing = [];
  for (const build of ["b101", "b102", "b103", "b104"]) {
    const bytes = await size(api, build);
    if (bytes === null) missing.push(build);
    else total += bytes;
  }
  return { total, missing };
}
