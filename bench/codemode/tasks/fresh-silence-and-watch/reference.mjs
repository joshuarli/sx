export default async function main(api) {
  await api.alerts.silence({ alert: "disk-full-db3", minutes: 30 });
  for (let check = 1; check <= 5; check++) {
    const { state } = await api.alerts.state({ alert: "disk-full-db3" });
    if (state === "resolved") return "resolved";
    if (check < 5) await api.sleep(15000);
  }
  await api.alerts.page({ alert: "disk-full-db3" });
  return "paged";
}
