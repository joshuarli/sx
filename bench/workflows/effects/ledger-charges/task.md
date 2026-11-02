A payments service at `{{pay_origin}}` holds invoices and a ledger of charges. Charge every open invoice exactly once for its amount, then report which invoices you charged.

1. List the invoices with the `payments.invoices` tool. Each has an `id`, a `customer`, an `amount_cents`, and a `status` of `open` or `paid`.
2. Charge each `open` invoice with the `payments.charge` tool, which takes the invoice `id` and `amount_cents` and returns the id of the charge. Every call that succeeds adds a charge to the ledger, and the ledger is the truth about what was charged.
3. The service sometimes answers a charge with 503 (unavailable). Such an answer does not say whether the charge was recorded. The `payments.charges` tool lists the charges the ledger holds for an invoice.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Charge every `open` invoice exactly once, for exactly its `amount_cents`.
- Never charge a `paid` invoice.
- Never charge an invoice twice. When the answer to a charge is unclear, look up the invoice's charges with `payments.charges` before charging again, and charge again only if the ledger holds none.

Finish by returning a JSON list of the ids of the invoices you charged, in the order the service listed them.
