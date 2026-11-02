## Programs: the process pack

`process.run` starts one granted program directly, with no shell, so each argument is its own string
and there are no pipes, globs, or variables. Its result is a record; a nonzero exit status is a value
to check, not a failure:

```sx
(let [out (call process.run {:program "/usr/bin/git" :args ["rev-parse" "HEAD"] :cwd "/srv/repo"})]
  (if (= (get out :status) 0)
    (some (text.trim (get out :stdout)))
    none))
```

Output usually ends with a newline: `text.trim` it before comparing, and `(text.split out "\n")` for
lines. `process.run` is a write, so it cannot run inside `par-map` or `par`; run commands with `map`
or `loop`, in order. When a working directory may be missing, wrap the call in `attempt`:
`(match (attempt (call process.run {...})) (ok out) ... (err e) ...)`.
