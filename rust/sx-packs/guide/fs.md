## Files: the fs pack

Paths are absolute strings: build them with `(str root "/" name)`. Read a whole file with
`fs.read-text`, list a directory with `fs.list`, and test for a path with `fs.stat`, which returns
`none` when nothing is there:

```sx
(let [root "/srv/notes"
      entries (call fs.list {:path root})
      names (keep (fn [e] (match (get e :kind) File (some (get e :name)) _ none)) entries)
      present (match (call fs.stat {:path (str root "/index.md")}) (some s) true none false)
      texts (map (fn [name] {:name name :text (call fs.read-text {:path (str root "/" name)})}) names)]
  {:files (count texts) :has-index present})
```

`fs.write-text` replaces the whole file with the text you give, so build the complete new text first.
Split text into lines with `(text.split s "\n")`; a file that ends in a newline gives a last `""`.
