A traffic router at `{{mesh_origin}}` sends a share of each service's traffic to its new version. Promote the new versions of three services, one service after another in this order: `checkout`, `search`, `billing`.

Tools: `canary.set-weight` sets the percentage of traffic (`percent`, 0 to 100) sent to the new version of a service (`name`), and `canary.health` returns the new version's `state`: `healthy`, `degraded`, or `starting`.

For each service, work through the stages `5`, `25`, `50`, `100` (percent) in that order:

1. Set the weight to the stage's percentage. The service sometimes refuses with "rate limited" (HTTP 429), which means nothing was changed. Set it again after waiting 100 milliseconds, at most 2 more times. Do not repeat after any other failure.
2. Then probe the health. Make the first probe right after the weight is set, and wait 50 milliseconds between probes. Keep a count of consecutive `healthy` answers and a count of consecutive `degraded` answers: a `healthy` answer adds one to the first count and resets the second to 0, a `degraded` answer adds one to the second count and resets the first to 0, and a `starting` answer resets both to 0.
   - When 3 consecutive `healthy` answers are counted, the stage `passed`. Make no further probe, and go on to the next stage.
   - When 2 consecutive `degraded` answers are counted, the stage `degraded`.
   - When the stage has had 12 probes and is neither, the stage `timeout`.
3. A stage that did not pass ends the rollout of that service: set its weight back to 0 (with the same rule for refusals as in step 1), once, right after the failing stage, and attempt no later stage of that service. The service's outcome is `rolled-back`. A service that passed all four stages has the outcome `promoted`. Then go on to the next service.

Return a JSON object with these fields:

- `services`: one object `{"service": NAME, "outcome": OUTCOME, "stages": STAGES, "final_weight": WEIGHT}` for each service in the order above, where `STAGES` has one object `{"percent": PERCENT, "result": RESULT, "probes": N}` for each stage that was attempted, in order, with `RESULT` one of `passed`, `degraded`, `timeout` and `N` the number of probes made in that stage, and `WEIGHT` is the weight the service was last set to
- `promoted`: the number of services with the outcome `promoted`
- `rolled_back`: the number with the outcome `rolled-back`
