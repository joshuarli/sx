A deploy service at `{{deploy_origin}}` rolls out new versions of services. Roll out these five, then report how each one ended:

| Service | Version |
| --- | --- |
| `api` | `1.4.0` |
| `web` | `2.1.0` |
| `worker` | `0.9.3` |
| `billing` | `3.0.0` |
| `search` | `1.1.1` |

1. Start a deployment for each service with the `deploy.start` tool, which takes the `service` and the `version`. It returns the deployment's `id`. Start each service's deployment exactly once: a successful start must never be repeated, because it would deploy twice. The service sometimes refuses a start with "rate limited" (HTTP 429). That refusal means nothing was started, so repeat that start after waiting 100 milliseconds, at most 2 more times. Do not repeat a start after any other failure.
2. Watch each deployment with the `deploy.status` tool, which returns its `status`: `running`, then `done` or `failed`. The service takes a few status requests to finish a deployment. Ask about a deployment's status only after its start succeeded, and ask again every 100 milliseconds. Stop asking about a deployment as soon as its status is `done` or `failed`. If a deployment's status is still `running` 1 second after your first status request about it, give up on it as timed out and stop asking about it. You may watch deployments one after another or at the same time.
3. Roll back each deployment whose status was `failed` with the `deploy.rollback` tool, once, after you saw it fail. Do not roll back a deployment that is `done` or timed out.

Return a JSON object with these fields:

- `deployments`: one object `{"service": SERVICE, "id": ID, "outcome": OUTCOME}` for each of the five services, in the order of the table above, with `OUTCOME` one of `done`, `rolled-back`, or `timeout`
- `done`: the number of deployments with outcome `done`
- `rolled_back`: the number with outcome `rolled-back`
- `timed_out`: the number with outcome `timeout`
