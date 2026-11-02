The zone `example.test` is served by a DNS service at `{{dns_origin}}`. A change set at `{{root}}/changes/cutover.json` has a `records` array of records to add for a cutover, each with `name`, `type`, `value`, and `ttl`, in the order they must be created.

1. Read the change set, and create its records in order with the `dns.create` tool, which returns the id of the new record. The service validates each record when it is created and can refuse it.
2. The `dns.records` tool lists the records in the zone, each with an `id`. The `dns.remove` tool deletes one record by its id.

The cutover is all or nothing. A zone with only some of the new records sends part of the traffic to the old addresses.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- When you finish, either every record of the change set exists in the zone exactly as listed, or the zone holds exactly the records it held when you started.
- If the service refuses a record, undo what you created: delete the records you created, by id.
- Never delete a record that existed before you started, even one with the same name as a record you created.
- Create nothing that is not in the change set.

Finish by returning a JSON object `{"applied": true|false, "refused": NAME}` where `NAME` is the name of the first record the service refused, or an empty string when none was refused.
