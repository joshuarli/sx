export default async function main(api) {
  let total = 0;
  let cursor = null;
  for (let page = 0; page < 10; page++) {
    const result = await api.staging.page({ table: "visits", cursor });
    if (result.rows.length > 0) total += (await api.warehouse.upsert({ table: "visits", rows: result.rows })).count;
    if (result.next === null) return total;
    cursor = result.next;
  }
  throw new Error("more than 10 pages");
}
