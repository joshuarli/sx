`yaml` reads and edits YAML text in place: `yaml.set` replaces one scalar's text and leaves every other
byte alone, so read the file, set each value, and write the result back. A path lists keys from the root,
and a sequence index is a string: `["spec" "containers" "0" "image"]`. Every scalar reads as a string,
so `(text.to-int (yaml.get doc ["replicas"]))` needs the value to be present: match the `Option` first.

```sx
(let [path "/srv/deploy/web.yaml"
      doc (call fs.read-text {:path path})
      replicas (match (yaml.get doc ["spec" "replicas"]) (some r) (match (text.to-int r) (some n) n none 1) none 1)
      edited (yaml.set doc ["spec" "replicas"] (str (* replicas 2)))
      _ (call fs.write-text {:path path :text edited})]
  replicas)
```

A file outside the supported subset (flow collections, anchors, block scalars, several documents) stops
the program, and `attempt` does not catch it, so leave such a file alone when the task says how to
recognize it.
