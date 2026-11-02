`json` reads JSON text by path, the way `yaml` does, in place of `JSON.parse`. A path lists object keys
from the root, and an array index is a string: `["dependencies" "0" "name"]`. `json.get` returns a
scalar as text, a number exactly as written, or `none` when the path is absent or `null`. `json.keys`
lists an object's keys in order, or an array's indexes, so walk an array with them. `json.kind` tells
`Absent`, `Null`, `Boolean`, `Number`, `String`, `Array`, and `Object` apart:

```sx
(def doc (call fs.read-text {:path "/srv/app/package.json"}))
(defn version-of [name] (json.get doc ["dependencies" name]))
(def deps (keep (fn [name] (match (version-of name) (some v) (some {:name name :version v}) none none))
                (json.keys doc ["dependencies"])))
{:name (match (json.get doc ["name"]) (some n) n none "") :deps deps}
```

Text that is not valid JSON is a refusal: wrap the read in `attempt` to handle a broken file, and it
gives `(err {:kind Invalid ...})`.
