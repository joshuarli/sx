export default async function main(api) {
  const key = await api.keys.create({ service: "billing" });
  for (let check = 1; check <= 12; check++) {
    const { state } = await api.keys.state({ id: key.id });
    if (state === "active") {
      await api.keys.revoke({ id: "k-old-17" });
      return { key: key.id, revoked: true };
    }
    if (check < 12) await api.sleep(5000);
  }
  return { key: key.id, revoked: false };
}
