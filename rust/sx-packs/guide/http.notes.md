### How the web service tools behave

- A response status of 2xx returns the decoded JSON body. Other statuses fail: 400 and 422 with
  `Invalid`, 401 and 403 with `PermissionDenied`, 404 with `NotFound`, 409 and 412 with `Conflict`, 429
  with `RateLimited`, 501 with `Failed`, and any other 5xx with `Unavailable`.
- A record result drops JSON fields it does not list when it is open (shown with `...`), and a missing
  listed field or a wrong type fails the call.
- Credentials the host configured are added to each request by the host. The program never sees or
  sends them.
- Requests go only to the granted origins with the granted methods; anything else fails with
  `PermissionDenied`. A request has 30 seconds.
