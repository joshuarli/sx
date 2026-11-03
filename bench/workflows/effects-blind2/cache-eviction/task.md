The disk cache under `{{root}}/cache` has grown past its budget. Delete the least valuable cached files until it fits, then report what you deleted.

1. A cache index service at `{{idx_origin}}` lists the cached entries with `cachex.entries`, one page at a time. Each page has `items` and a `next` cursor (absent on the last page); pass `next` back as `cursor` to get the next page, starting with no cursor. An item has a `key`, a `path` relative to `{{root}}/cache`, its size in `bytes`, and the number of `hits` it has served. The index is the truth about sizes; do not measure the files. Reading a page sometimes fails with 503; reading is safe to repeat.
2. The cache budget is the number `cache_bytes` in the JSON file `{{root}}/config/limits.json`.
3. The file `{{root}}/config/pins.txt` lists pinned keys, one per line. Blank lines and lines that start with `#` are not keys, and a key may have spaces around it.
4. `fs.delete` removes one file.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- The cache holds every indexed entry, so its size is the sum of the `bytes` of all entries, pinned ones included. If that sum is within the budget, delete nothing.
- Otherwise evict entries one at a time, deleting the file at `{{root}}/cache/` plus the entry's `path`, and stop as soon as the remaining sum is within the budget. Evict in this order: fewest `hits` first; among equal hits the larger `bytes` first; among those the smaller `key` first (plain character order).
- Never evict a pinned entry. Skip it and go on to the next one.
- Delete only the files of evicted entries. Other files lie in the cache directory and elsewhere under `{{root}}`; leave all of them alone.

Finish by returning a JSON list of the keys you evicted, in the order you evicted them.
