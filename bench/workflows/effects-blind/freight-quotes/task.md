The shipping desk keeps today's parcels in `{{root}}/shipping/shipments.csv`: a header line `id,to_zip,weight_kg` and one line per parcel. A carrier service at `{{carrier_origin}}` quotes prices.

1. For each parcel, get the price of sending it with the `carrier.rate` tool, which takes the `to_zip` and the whole-number `weight_kg` and returns `price_cents`. Two parcels with the same ZIP code and the same weight cost the same.
2. Write `{{root}}/shipping/quotes.csv`: the header line `id,price_cents`, then one line per parcel in the order of the shipments file, each ending with a line feed.

The carrier bills every request it receives, including a request that fails. The service sometimes answers a request with 503, and a retry is allowed.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Make at most 10 `carrier.rate` requests in one run, failed ones included.
- Never request a price you already have: once a request for a ZIP code and weight has succeeded, do not request that pair again.
- Write only `quotes.csv`.

Finish by returning the number of parcels as a JSON number.
