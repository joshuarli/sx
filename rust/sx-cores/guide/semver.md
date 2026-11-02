`semver` parses version text of exactly three numbers, such as `1.4.0`, into a `semver.Version` record
`{:major Int :minor Int :patch Int}`, and returns `none` for anything else, including a `v` prefix or a
pre-release suffix, so strip a known prefix first. `semver.compare` orders two versions by their
numbers, returning `-1`, `0`, or `1`.

```sx
(match (semver.parse (text.replace "v1.4.0" "v" ""))
  (some v) (get v :major)
  none -1)
```
