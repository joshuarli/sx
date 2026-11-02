const TENTHS = 10n;
const THOUSANDTHS = 1000n;

function scaled(text: string, places: number): bigint {
  const [whole = "0", fraction = ""] = text.trim().split(".");
  return BigInt(whole + fraction.padEnd(places, "0").slice(0, places));
}

function lineCents(fields: string[]): bigint {
  const quantity = BigInt(fields[1]!.trim());
  const price = scaled(fields[2]!, 3);
  const discount = scaled(fields[3]!, 1);
  const numerator = quantity * price * (100n * TENTHS - discount);
  const unit = THOUSANDTHS * TENTHS;
  return (numerator + unit / 2n) / unit;
}

function money(cents: bigint): string {
  const negative = cents < 0n;
  const digits = (negative ? -cents : cents).toString().padStart(3, "0");
  return `${negative ? "-" : ""}${digits.slice(0, -2)}.${digits.slice(-2)}`;
}

export default async function main(api: Api) {
  const dir = "{{root}}/invoices";
  const entries = await api.fs.list({ path: dir });
  const names = entries
    .filter((e) => e.kind === "File" && e.name.endsWith(".csv"))
    .map((e) => e.name)
    .sort();
  const invoices = await Promise.all(
    names.map(async (name) => {
      const rows = (await api.fs.readText({ path: `${dir}/${name}` }))
        .split("\n")
        .filter((line) => line.trim() !== "")
        .map((line) => line.trim().split(","));
      const [head, ...lines] = rows;
      return {
        id: head![1]!,
        customer: head![2]!,
        stated: scaled(head![3]!, 2),
        computed: lines.reduce((sum, fields) => sum + lineCents(fields), 0n),
      };
    }),
  );
  const byCustomer = new Map<string, { invoices: number; total: bigint }>();
  for (const invoice of invoices) {
    const entry = byCustomer.get(invoice.customer) ?? { invoices: 0, total: 0n };
    entry.invoices++;
    entry.total += invoice.computed;
    byCustomer.set(invoice.customer, entry);
  }
  const text = (a: string, b: string) => (a < b ? -1 : a > b ? 1 : 0);
  return {
    customers: [...byCustomer]
      .sort((a, b) => text(a[0], b[0]))
      .map(([customer, entry]) => ({ customer, invoices: entry.invoices, total: money(entry.total) })),
    mismatched: invoices
      .filter((i) => i.stated !== i.computed)
      .sort((a, b) => text(a.id, b.id))
      .map((i) => ({ invoice: i.id, stated: money(i.stated), computed: money(i.computed) })),
  };
}
