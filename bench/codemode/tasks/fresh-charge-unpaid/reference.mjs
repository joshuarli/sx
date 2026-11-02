const DELAYS = [2000, 4000, 8000];

async function charge(api, invoice) {
  for (let attempt = 0; ; attempt++) {
    try {
      await api.payments.charge({ invoice: invoice.id, amount: invoice.amount, key: "charge-" + invoice.id });
      return null;
    } catch (error) {
      if (error.kind === "Conflict") return null;
      if (!["Timeout", "Unavailable"].includes(error.kind) || attempt === DELAYS.length) return error.kind;
      await api.sleep(DELAYS[attempt]);
    }
  }
}

export default async function main(api) {
  const paid = [];
  const failed = [];
  for (const invoice of await api.billing.unpaid()) {
    const kind = await charge(api, invoice);
    if (kind === null) paid.push(invoice.id);
    else failed.push({ id: invoice.id, kind });
  }
  return { paid, failed };
}
