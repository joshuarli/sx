export default async function main(api) {
  const [staging, prod] = await Promise.all([api.flags.all({ env: "staging" }), api.flags.all({ env: "prod" })]);
  const names = [...new Set([...Object.keys(staging), ...Object.keys(prod)])].sort();
  return names
    .map((flag) => ({ flag, staging: staging[flag] ?? null, prod: prod[flag] ?? null }))
    .filter((row) => row.staging !== row.prod);
}
