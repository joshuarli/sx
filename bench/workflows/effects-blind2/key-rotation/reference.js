const KEYS = tools.mcp__tools__vault_keys;
const CONSUMERS = tools.mcp__tools__vault_consumers;
const FREEZES = tools.mcp__tools__vault_freezes;
const CREATE = tools.mcp__tools__vault_create_key;
const USE = tools.mcp__tools__vault_use_key;
const REVOKE = tools.mcp__tools__vault_revoke_key;

const keys = (await KEYS({})).structuredContent.result;
const consumers = (await CONSUMERS({})).structuredContent.result;
const frozen = new Set((await FREEZES({})).structuredContent.result.map((f) => f.consumer));
const rotated = [];
for (const label of ["billing", "reports"]) {
  const old = keys.find((k) => k.label === label && k.state === "active").id;
  const users = consumers.filter((c) => c.key_id === old).map((c) => c.id);
  if (users.some((u) => frozen.has(u))) continue;
  let created = await CREATE({ label });
  let fresh;
  if (created.isError) {
    const now = (await KEYS({})).structuredContent.result;
    fresh = now.find((k) => k.label === label && k.state === "active" && k.id !== old);
    if (!fresh) created = await CREATE({ label });
  }
  const id = fresh ? fresh.id : created.structuredContent.id;
  for (const user of users) {
    for (let attempt = 0; ; attempt++) {
      const answer = await USE({ id: user, key_id: id });
      if (!answer.isError) break;
      if (attempt === 3) throw new Error(`could not switch ${user}`);
    }
  }
  await REVOKE({ id: old });
  rotated.push({ label, old, new: id });
}
return rotated;
