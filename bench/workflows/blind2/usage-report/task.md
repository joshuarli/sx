A billing service at `{{billing_origin}}` knows the accounts of a hosting company. Build a ranking of the accounts with the highest error rates and write it to `{{root}}/usage.csv`.

1. List the accounts with the `accounts.page` tool, which takes `limit` and `offset` and returns the `items` of that page and the `total` number of accounts. Request pages one after another with `limit` 20 and `offset` 0, 20, 40, and so on, and stop when the next offset would not be below `total`. Do not request any other page.
2. Ask about the usage of every listed account exactly once with the `accounts.usage` tool, which takes the account `id` and returns `requests` and `errors`. Ask about at most 4 accounts at the same time. An account the service reports as not found (HTTP 404) is `closed`: leave it out of the ranking.
3. The service sometimes refuses a request, for either tool, with "rate limited" (HTTP 429) or "unavailable" (HTTP 503). Repeat a refused request at most 2 more times, waiting 30 milliseconds before the first repeat and 60 before the second. Do not repeat a request after any other failure, and never ask about an account again after you got its usage. An account whose usage request is still refused after the last repeat is `unreachable`: leave it out of the ranking as well. If a page request is still refused after the last repeat, give up.
4. The rate of an account is its `errors` times 1000 divided by its `requests`, dropping any fraction. An account with 0 requests has rate 0. Rank the accounts left, highest rate first. Among equal rates the account with more `requests` comes first, and among those the account whose `id` is smaller in ascending text order.
5. Write `{{root}}/usage.csv` with exactly these lines, each ending with `\n`, and no other text. The first line is `id,name,plan,requests,errors,rate`. Then one line for each of the 10 best ranked accounts, in rank order, with the fields in that order and separated by commas. A `name` that contains a comma or a double quote is written inside double quotes, with each double quote inside it doubled. No other field is quoted.

Return a JSON object with these fields:

- `listed`: the number of accounts the pages listed
- `closed`: the `id`s of the closed accounts, in ascending text order
- `unreachable`: the `id`s of the unreachable accounts, in ascending text order
- `requests`: the sum of `requests` over the accounts in the ranking, all of them and not only the 10 written
- `top`: `[{"id": ID, "rate": RATE}, ...]` for the 10 accounts written, in rank order
