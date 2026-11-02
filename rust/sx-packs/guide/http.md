## Web services: typed endpoints and the http pack

A typed endpoint is a tool like any other: its input fields fill the URL path and query, and its
result is the decoded JSON body. An error status is a failure with a kind, so handle it with `attempt`
and `match` on the kind:

```sx
(match (attempt (call ci.checks {:repo "api" :sha "abc123"}))
  (ok checks) (get checks :state)
  (err e) (match (get e :kind) NotFound "unknown" _ "error"))
```

Retry what the task says to retry with stated delays, such as
`(retry {:delays [1s 2s] :on [RateLimited Unavailable]} (call ...))`. A write that is not idempotent may
be retried only on `RateLimited` and other refusal kinds.
