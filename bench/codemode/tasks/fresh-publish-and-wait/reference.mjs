export default async function main(api) {
  const upload = await api.registry.publish({ name: "fastq", version: "3.2.0" });
  const delays = [1000, 2000, 4000, 8000];
  for (let checks = 1; ; checks++) {
    const { state } = await api.registry.upload({ id: upload.id });
    if (state === "live" || state === "rejected" || checks === 5) return { state, checks };
    await api.sleep(delays[checks - 1]);
  }
}
