export default async function main(api) {
  const content = await api.files.read({ path: "stock.csv" });
  const [header, ...rest] = content.split("\n");
  const columns = header.split(",");
  const qty = columns.indexOf("qty");
  const rows = rest.filter((line) => line !== "").map((line) => line.split(","));
  const total = rows.reduce((sum, row) => sum + Number(row[qty]), 0);
  return { columns, rows: rows.length, total };
}
