A warehouse service at `{{wh_origin}}` tracks stock and purchase orders. Restock every SKU that has fallen below its reorder level, once.

1. List the stock with the `warehouse.stock` tool. Each SKU has `on_hand`, `reorder_below`, and `target`. A SKU needs restocking when `on_hand` is below `reorder_below`.
2. A SKU that already has an open purchase order is waiting for a delivery: do not order it again. The `warehouse.orders` tool lists the purchase orders of one SKU; each has a `status` of `open` or `received`.
3. Place one purchase order for every other SKU that needs restocking, with the `warehouse.order` tool, for `target - on_hand` units. Every call that succeeds adds a purchase order, and the warehouse's list of purchase orders is the truth about what was ordered.
4. The service sometimes answers an order with an error. A 429 means it did not place the order. A 5xx does not say whether the order was placed.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Every SKU that needs restocking and has no open purchase order ends with exactly one new purchase order, for exactly `target - on_hand` units.
- Never order a SKU that does not need restocking, or one that already has an open purchase order.
- Never order a SKU twice. When the answer to an order is unclear, look up the SKU's purchase orders with `warehouse.orders` before ordering again, and order again only if no new open order exists.

Finish by returning a JSON list of the SKUs you ordered, in the order of the stock listing.
