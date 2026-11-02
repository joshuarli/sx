An inventory service offers two tools, `lookup-item` (reads one item's catalog entry by `sku`) and `reserve-stock` (reserves `quantity` units of one item, with an optional `priority` of `normal` or `rush`, and returns a `reservation` id and the stock `remaining`). A tool call for a sku the catalog does not know fails.

Process this order, a list of `(sku, quantity)` lines: `E-500` x 2, `A-100` x 3, `D-404` x 2, `C-300` x 5, `B-200` x 1.

1. Look up each sku in the order exactly once. The lookups may run concurrently. A sku whose lookup fails is `unknown`.
2. Handle the skus in ascending order of the sku text, one at a time. Skip an unknown sku. Skip a sku whose `stock` is less than the ordered quantity: it is `insufficient`. Otherwise call `reserve-stock` for the ordered quantity once, with `priority` set to `rush` when the item's `tags` include `perishable`, and with no priority otherwise.

Return a JSON object with three fields:

- `reserved`: one object `{"sku": SKU, "reservation": ID, "cents": N}` for each reserved line, in the order the reservations were made, with `N` the item's `cents` price times the ordered quantity
- `rejected`: one object `{"sku": SKU, "reason": REASON}` for each skipped line, in ascending sku order, with `REASON` either `unknown` or `insufficient`
- `total`: the sum of the `cents` of the reserved lines
