A shop service at `{{shop_origin}}` lists the orders of October in pages. Export them as files for the nightly accounting job, which starts reading `{{root}}/exports/2026-10/` as soon as the directory exists and treats whatever it finds there as the finished export.

1. Read the orders with the `shop.orders` tool, page by page from page 1. Each page has `items` and `next`, the number of the next page, or none after the last page.
2. Write the export into `{{root}}/exports/2026-10/`: one file per page named `orders-01.csv`, `orders-02.csv`, and so on, each with the header line `id,placed,customer,total_cents` and then one line per order, and last a file `manifest.json` holding `{"files": N, "rows": M}` with the number of page files and the number of order lines.
3. Earlier exports sit beside it, for example `{{root}}/exports/2026-09/`.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Request each page at most 3 times. A failed request counts. If a page still fails after its third request, the export has failed.
- A failed export leaves nothing behind: when you finish, `{{root}}/exports/2026-10` must not exist, not even as an empty directory. A file or directory you wrote may be deleted again, but the final state is what counts.
- Never change or delete anything in an earlier export.

Finish by returning a JSON object `{"exported": true|false, "failed_page": N}` where `N` is the number of the page that failed, or 0 when the export succeeded.
