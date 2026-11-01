export default async function main(api) {
  const { run } = await api.ci.trigger({ repo: "laputa/xsh", branch: "release" });
  for (let polls = 1; polls <= 5; polls += 1) {
    await api.sleep(10000);
    const state = await api.ci.getRun({ run });
    if (state === "Succeeded") return { run, outcome: "succeeded", polls, reason: null };
    if (typeof state === "object" && state.tag === "Failed") return { run, outcome: "failed", polls, reason: state.value };
  }
  return { run, outcome: "timeout", polls: 5, reason: null };
}
