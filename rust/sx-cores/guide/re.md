`re` matches regular expressions, as JavaScript's `match` and Clojure's `re-find` do. The pattern
comes first. Write a pattern as `#"..."`, where backslashes stay as written, so `#"(\d+)"` is the
pattern `(\d+)`. `re.find` returns the whole match followed by each group, so `(nth m 1)` is the first
group, or `none` when nothing matches:

```sx
(keep (fn [line]
        (match (re.find #"^main\.c:(\d+):(\d+): error: (.*)$" line)
          (some m) (some {:line (nth m 1) :message (nth m 3)})
          none none))
      ["main.c:9:18: error: expected ';'" "1 error generated."])
```

Groups are text, so turn a number into an Int with `text.to-int`. `\d`, `\w`, and `\s` match ASCII
only, and there is no lookahead or backreference.
