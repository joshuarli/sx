A warehouse service at `{{wh_origin}}` holds the system's stock levels. Last week's cycle count is a CSV file at `{{root}}/counts/cycle.csv`. Bring the system's levels in line with the count by adjusting them, then report what you adjusted.

1. The count file has a header line `sku,counted,counter` and one line per counted SKU. Fields may have spaces around them, there may be blank lines, and a SKU may be counted more than once: the last line for a SKU is the count that stands. The file may name SKUs the warehouse does not know.
2. `wh.stock` lists every SKU with its `on_hand` quantity and a `frozen` flag, and `wh.level` reads one SKU. A frozen SKU is under audit.
3. `wh.adjust` changes a SKU's `on_hand` by a signed `delta` and returns the new `on_hand`. Every call that succeeds adds the delta to the level again. The service does not refuse an adjustment of a frozen SKU.
4. The service sometimes answers an adjustment with 503 (unavailable) or 429 (rate limited). A 429 means the adjustment was not applied. A 503 does not say whether it was.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- For every counted SKU that the warehouse knows, is not frozen, and whose level differs from its count, adjust it so that its `on_hand` ends equal to the count that stands. Do not adjust any other SKU, and never adjust by a delta of zero.
- Adjust each SKU in one call when you can. When the answer to an adjustment is unclear, read the SKU with `wh.level` before adjusting again, and adjust only by what is still missing.
- No adjustment may move a SKU away from its count.

Finish by returning a JSON list with one object per SKU you adjusted, holding its `sku` and the `delta` that was needed, in SKU order.
