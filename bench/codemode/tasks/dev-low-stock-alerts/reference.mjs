export default async function main(api) {
  const items = await api.inventory.items({});
  let sent = 0;
  for (const item of items) {
    if (item.stock < 5) {
      await api.mail.send({ to: "ops@example.com", subject: `Low stock: ${item.sku}` });
      sent += 1;
    }
  }
  return sent;
}
