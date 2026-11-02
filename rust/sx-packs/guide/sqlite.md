## Databases: declared SQL statements

Each statement the host declares is a tool. Its input fields bind the statement's `:name` parameters,
and its result is the number of changed rows or a list of row records:

```sx
(def _ (call store.save-order {:id "o-1" :cents 1200}))
(def rows (call store.orders-over {:cents 1000}))
(map (fn [r] (get r :id)) rows)
```

Each call is its own transaction. Run writes in the order the task gives, with `map`, `def`, or `loop`.
