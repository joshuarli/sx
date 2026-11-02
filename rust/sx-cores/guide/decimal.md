`decimal` does exact money arithmetic. Parse each amount, which returns an `Option`, combine with
`decimal.add`, `decimal.mul`, and the others, and turn the result into text with `decimal.format` for a
fixed number of places. Rounding modes are `HalfEven`, `HalfUp`, `Down`, `Up`, `Floor`, and `Ceiling`.
Compare with `decimal.compare`, never `<`:

```sx
(def amounts (keep decimal.parse ["12.50" "0.75" "3"]))
(def total (reduce decimal.add (decimal.from-int 0) amounts))
{:total (decimal.format total 2) :over-ten (= (decimal.compare total (decimal.from-int 10)) 1)}
```
