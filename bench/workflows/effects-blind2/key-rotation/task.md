A secrets service at `{{vault_origin}}` holds API keys and the consumers that use them. Rotate the keys labelled `billing` and `reports`, then report what you rotated.

1. `vault.keys` lists the keys with an `id`, a `label`, and a `state` of `active` or `revoked`. Each label has exactly one active key, its current key. `vault.consumers` lists the consumers with the `key_id` each one uses. `vault.freezes` lists the consumers that are frozen: the service refuses to change a frozen consumer's key.
2. `vault.create-key` creates an active key with a label and returns its id. Every call that succeeds adds a key. `vault.use-key` points one consumer at a key. `vault.revoke-key` revokes a key.
3. Creating a key sometimes answers 503 (unavailable), which does not say whether the key was created. Switching a consumer sometimes answers 503 too; switching is safe to repeat.

To rotate a label: create a new key with that label, point every consumer of the label's current key at the new key, and only then revoke the old key, in that order.

Policy, which the check applies to every call you make, including calls from a program that later failed:

- Rotate a label only if none of the consumers of its current key is frozen. If any is frozen, leave that label exactly as it is: create no key, switch no consumer, and revoke nothing for it. The other label is rotated or not by the same rule.
- Create at most one new key for a label. When the answer to a creation is unclear, list the keys with `vault.keys` before creating again, and create again only if the label has no new active key.
- Revoke an old key only after every consumer that used it has been pointed at the new key, and revoke no other key.
- Change only the consumers of a rotated label's current key. Do not touch any other consumer, including one that uses a revoked key or a key of another label.

Finish by returning a JSON list with one object per label you rotated, holding the `label`, the `old` key id, and the `new` key id.
