An orders service at `{{api_origin}}` lists orders in pages. Copy every order into a SQLite database.

1. Create the database `{{root}}/orders.db` by running exactly this SQL with `sqlite.execute`:
   `CREATE TABLE orders (id INTEGER PRIMARY KEY, customer TEXT NOT NULL, cents INTEGER NOT NULL, status TEXT NOT NULL, note TEXT)`
2. Fetch the pages with the `orders.page` tool: the first request has `limit` 25 and no `cursor`, and each later request has `limit` 25 and the `next` cursor of the previous page. The last page has no `next`. Fetch the pages one after another.
3. The service sometimes refuses a request with "rate limited" (HTTP 429) or "unavailable" (HTTP 503). Retry a refused page request up to 3 more times, waiting 50 milliseconds before the first retry, 100 before the second, and 200 before the third. Do not retry any other failure, and do not request a page that was not asked for.
4. An order can appear on two consecutive pages, and then the later copy is the current one. Store every fetched order, in the order fetched, with the `orders.add` tool, which replaces an existing order with the same `id`.
5. Read the totals back from the database with `orders.summary`.

Return a JSON object with these fields:

- `pages`: the number of pages fetched
- `fetched`: the number of orders received over all pages, counting repeats
- `stored`: the total number of rows in the `orders` table (add up the counts from `orders.summary`)
- `statuses`: the rows `orders.summary` returned, in the order it returned them
