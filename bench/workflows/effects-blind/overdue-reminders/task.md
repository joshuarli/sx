A billing service at `{{billing_origin}}` holds invoices, customers, and a mail relay. Remind the customers who have overdue invoices.

1. List the invoices with the `billing.invoices` tool. Each has an `id`, a `customer_id`, an `amount_cents`, and a `status` of `open`, `overdue`, or `paid`.
2. Look up a customer with the `billing.customer` tool, which returns the customer's `email` and `do_not_contact`.
3. Send a reminder with the `billing.email` tool, which takes `to`, `subject`, and `body`. Every call that succeeds sends a real message, and the relay's log is the truth about what was sent.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Send each customer who has at least one overdue invoice exactly one email, to the customer's `email`, whose body names every overdue invoice id of that customer, and no other invoice id. A customer with several overdue invoices gets one email, not one per invoice.
- Never email a customer whose `do_not_contact` is true, or a customer with no overdue invoice.
- The relay is billed per message: send at most 4 emails in one run.

Finish by returning a JSON list of the ids of the customers you emailed, in the order of their first overdue invoice in the listing.
