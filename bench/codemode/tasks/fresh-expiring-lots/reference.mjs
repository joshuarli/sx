export default async function main(api) {
  const lots = await api.batches.list();
  const soon = lots.filter((l) => l["days-left"] <= 14 && l.quantity > 0);
  soon.sort((a, b) => a["days-left"] - b["days-left"] || (a.lot < b.lot ? -1 : 1));
  const byWarehouse = {};
  for (const lot of soon) (byWarehouse[lot.warehouse] ??= []).push(lot.lot);
  return byWarehouse;
}
