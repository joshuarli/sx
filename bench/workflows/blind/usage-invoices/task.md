The billing service at `{{billing_origin}}` knows the accounts and their usage for the month. Charge each account for its usage and keep a local record of the invoices.

1. List the accounts with the `billing.accounts` tool, page by page: the first request is for page 1, and each answer has the `pages` count, so request every page from 1 up to `pages` one after another and no other page. Each account has an `id` and a `plan`: `free`, `standard`, or `enterprise`.
2. The local record is the `invoices` table of the SQLite database `{{root}}/ledger.db`, which already exists. Read the accounts it already holds with `ledger.all`. Do not charge an account that is in it, and do not ask for its usage.
3. An account on the `free` plan is never charged: do not ask for its usage. For every other account not in the local record, ask for its usage with `billing.usage`, with at most 5 requests in flight at once and one request per account unless it is repeated as follows:
   - "not found" (HTTP 404) means the account is `unknown`: it is not charged and the request is not repeated;
   - the service sometimes answers "unavailable" (HTTP 503). Ask again after waiting 50 milliseconds, and once more after waiting another 50 milliseconds if it fails again. If the third request fails too, the account is `failed` and is not charged. Do not repeat a request after any other failure.
4. Compute the charge of an account from its usage events, each with a `metric` and a `quantity`. Add up the quantities of the events of each metric. Only the metrics `api` and `storage` count; others are ignored. All amounts are whole cents.
   - `standard` plan: the first 1000 `api` units cost nothing, the units from 1001 up to 10000 cost 3 cents each, and every unit above 10000 costs 2 cents each. `storage` units cost 50 cents each.
   - `enterprise` plan: the first 10000 `api` units cost nothing, and every unit above costs 1 cent. `storage` units cost 30 cents each. When the subtotal is more than 0 but less than 5000, it is raised to 5000.
   - The subtotal is the `api` charge plus the `storage` charge, after the rules of the plan. An account whose subtotal is 0 has `no_charge` and gets no invoice.
   - The tax is 7 percent of the subtotal, rounded to a whole cent with halves rounded up. The amount to invoice is the subtotal plus the tax.
5. Create the invoices one at a time, in ascending order of account `id`, with the `billing.invoice` tool, which takes the `account` and the amount in `cents` and returns the invoice's `invoice` id. Never create two invoices for one account. The service sometimes refuses with "rate limited" (HTTP 429), which means nothing was created: ask again after waiting 100 milliseconds, at most 2 more times. A "conflict" (HTTP 409) means the service already has an invoice for the account: the account is `already`, and nothing is recorded locally. Any other failure makes the account `failed`, and the request is not repeated.
6. Record each invoice you created with `ledger.record`, right after it was created.

Return a JSON object with these fields, each list of account ids in ascending order:

- `accounts`: the number of accounts listed
- `free`: the number of accounts on the `free` plan
- `ledgered`, `unknown`, `no_charge`, `already`, `failed`: the ids of the accounts in that state
- `invoiced`: one object `{"account": ID, "cents": CENTS, "invoice": INVOICE}` for each invoice created
- `total_cents`: the sum of the `cents` of the invoices created
