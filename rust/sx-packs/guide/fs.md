## Files: the fs pack

Paths are absolute strings: build them with `(str root "/" name)`. Read a whole file with
`fs.read-text`, list a directory with `fs.list`, and test for a path with `fs.stat`, which returns
`none` when nothing is there:

```sx
(def root "/srv/notes")
(defn file-name [e] (match (get e :kind) File (some (get e :name)) _ none))
(def names (keep file-name (call fs.list {:path root})))
(def has-index (match (call fs.stat {:path (str root "/index.md")}) (some s) true none false))
(defn read-note [name] {:name name :text (call fs.read-text {:path (str root "/" name)})})
(def notes (map read-note names))
{:files (count notes) :has-index has-index}
```

`fs.write-text` replaces the whole file with the text you give, so build the complete new text first.
Split text into lines with `(text.split s "\n")`; a file that ends in a newline gives a last `""`.
