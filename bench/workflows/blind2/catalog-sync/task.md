A supplier service at `{{supplier_origin}}` publishes a numbered log of changes to its product catalog. Bring the local database `{{root}}/catalog.db` up to date with it. The database already exists and holds the products and the number of the last change that was applied.

1. Read the number of the last applied change with `sync.get-version`.
2. Fetch changes with the `changes.page` tool, which takes `since` and `limit` and returns the `changes` with a `seq` above `since`, in ascending `seq` order, the `version` (the highest `seq` in this response), and `more`, which is true when further changes exist. Use `limit` 10 in every request. The first request has `since` equal to the number you read, and each later request has `since` equal to the `version` of the previous response. Fetch the pages one after another, and stop after the page whose `more` is false.
3. A request is sometimes refused with "unavailable" (HTTP 503) or "rate limited" (HTTP 429). Repeat a refused request at most 2 more times, waiting 40 milliseconds before the first repeat and 80 before the second. Do not repeat a request after any other failure. When a page is still refused after the last repeat, stop fetching: keep what was applied so far, and the sync is not completed.
4. Apply a page like this. A page can hold several changes for one product (`sku`); of those, keep only the one with the highest `seq`, and ignore the others. Apply the kept changes in ascending `seq` order. A change whose `op` is `upsert` is applied with `products.upsert`, which takes `sku`, `name`, `price_cents`, and `stock` (they are present in an upsert) and creates the product or replaces its fields and marks it active. A change whose `op` is `delete` is applied with `products.discontinue`, which takes the `sku` and marks the product inactive; it does nothing when no such product exists, and it still counts as applied.
5. After all kept changes of a page are applied, and before the next request, store the page's `version` with `sync.set-version`. Do this once per page, and never store a version for a page that was not applied.
6. At the end read the number of active products with `products.active-count` and the stored version again with `sync.get-version`.

Return a JSON object with these fields:

- `since`: the number you read in step 1
- `pages`: the number of pages that were fetched and applied
- `applied`: the number of kept changes that were applied
- `upserted`: how many of those were upserts
- `discontinued`: how many of those were deletes
- `version`: the stored version read in step 6
- `active`: the number of active products read in step 6
- `completed`: true when the page with `more` false was applied, otherwise false
