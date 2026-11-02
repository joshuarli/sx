Text functions take the text first: `(text.split s ",")`, `(text.starts-with? s "v")`,
`(text.replace s "old" "new")`. `text.to-int` returns an `Option<Int>`, `none` for anything that is not
an integer, so match it: `(match (text.to-int s) (some n) n none 0)`. `(str ...)` joins values into text.
