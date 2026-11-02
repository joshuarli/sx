export default async function main(api: Api) {
  const customers = await api.customers.all({});
  const normalize = (email: string) => email.trim().toLowerCase();
  const groups = new Map<string, typeof customers>();
  for (const customer of customers) {
    const key = normalize(customer.email);
    groups.set(key, [...(groups.get(key) ?? []), customer]);
  }
  const plans = [...groups.values()]
    .filter((members) => members.length > 1)
    .map((members) => {
      const ordered = [...members].sort((a, b) => a.created - b.created || a.id - b.id);
      return { survivor: ordered[0]!.id, removed: ordered.slice(1).map((c) => c.id).sort((a, b) => a - b) };
    })
    .sort((a, b) => a.survivor - b.survivor);
  const merged = [];
  for (const plan of plans) {
    let orders = 0;
    let notesMoved = 0;
    let notesDropped = 0;
    for (const from of plan.removed) {
      orders += await api.orders.move({ from_id: from, to_id: plan.survivor });
      for (const note of await api.notes.forCustomer({ customer_id: from })) {
        try {
          await api.notes.move({ id: note.id, to_id: plan.survivor });
          notesMoved++;
        } catch (error) {
          if ((error as CallError).kind !== "Conflict") throw error;
          await api.notes.delete({ id: note.id });
          notesDropped++;
        }
      }
      await api.customers.delete({ id: from });
    }
    merged.push({
      survivor: plan.survivor,
      removed: plan.removed,
      orders,
      notes_moved: notesMoved,
      notes_dropped: notesDropped,
    });
  }
  const gone = new Set(plans.flatMap((plan) => plan.removed));
  const remaining = customers.filter((c) => !gone.has(c.id));
  const stale = remaining.filter((c) => c.email !== normalize(c.email));
  for (const customer of stale) await api.customers.setEmail({ id: customer.id, email: normalize(customer.email) });
  return { merged, emails_changed: stale.map((c) => c.id), customers: remaining.length };
}
