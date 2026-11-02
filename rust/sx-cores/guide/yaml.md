`yaml` reads and edits YAML text in place: `yaml.set` replaces one scalar's text and leaves every other
byte alone, so read the file, set each value, and write the result back. A path lists keys from the root,
and a sequence index is a string: `["spec" "containers" "0" "image"]`. Every scalar reads as a string,
so `(text.to-int (yaml.get doc ["replicas"]))` needs the value to be present: match the `Option` first.

```sx
(def path "/srv/deploy/web.yaml")
(def doc (call fs.read-text {:path path}))
(defn as-int [text] (match (text.to-int text) (some n) n none 1))
(def replicas (match (yaml.get doc ["spec" "replicas"]) (some r) (as-int r) none 1))
(def _ (call fs.write-text {:path path :text (yaml.set doc ["spec" "replicas"] (str (* replicas 2)))}))
replicas
```

A file outside the supported subset (flow collections, anchors, block scalars, several documents) is a
refusal: inside an `attempt` it gives `(err {:kind Invalid :message ...})`, and outside one it stops the
program.
