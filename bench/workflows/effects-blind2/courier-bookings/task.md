A depot service at `{{ship_origin}}` holds customer orders and courier bookings. Book the courier for every order that is ready to ship, then report what you booked.

1. `ship.orders` lists the orders in the order the depot keeps them. Each has an `id`, a `customer` (a customer id), a `weight_g`, and a `status` of `ready`, `hold`, or `shipped`.
2. `ship.customers` lists the customers. Each has an `id`, a `zone`, and a `blocked` flag.
3. `ship.book` creates a booking from a customer id, a zone, a list of order ids, and a total weight in grams, and returns the id of the booking. Every call that succeeds adds a booking to the depot, and the depot's bookings are the truth about what is booked. `ship.bookings` lists the bookings the depot holds for one customer.
4. The depot sometimes answers a booking with 503 (unavailable) or 429 (rate limited). A 429 means the booking was not recorded. A 503 does not say whether it was.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Book every `ready` order of every customer who is not `blocked`, exactly once. Never book an order that is on `hold` or `shipped`, and never book anything for a blocked customer.
- A booking holds orders of one customer only, and at most 4 of them. Take a customer's ready orders in the order the depot lists them: the first four go in the first booking, the next four in the second, and so on.
- A booking's zone is its customer's zone, and its `weight_g` is the sum of the weights of its orders.
- Never book an order twice. When the answer to a booking is unclear, look up the customer's bookings with `ship.bookings` before booking again, and book again only if the depot holds no booking with exactly those orders.

Finish by returning a JSON list with one entry per booking you made, in booking order: an object with the customer id and the list of order ids.
