async function runStage(api, stage) {
  const run = await api.etl.start({ stage, dataset: "orders" });
  for (let check = 1; check <= 6; check++) {
    const { state } = await api.etl.state({ id: run.id });
    if (state === "succeeded" || state === "failed") return state;
    if (check < 6) await api.sleep(20000);
  }
  return "timeout";
}

export default async function main(api) {
  const extract = await runStage(api, "extract");
  const load = extract === "succeeded" ? await runStage(api, "load") : "skipped";
  return { extract, load };
}
