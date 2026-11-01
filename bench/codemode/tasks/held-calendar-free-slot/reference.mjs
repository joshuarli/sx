export default async function main(api) {
  const users = ["ana", "ben", "chi"];
  const lists = await Promise.all(users.map((user) => api.calendar.busy({ user, day: "2026-03-12" })));
  const blocks = lists.flat().sort((a, b) => a.start - b.start);
  let cursor = 540;
  for (const block of blocks) {
    if (block.start - cursor >= 45) break;
    cursor = Math.max(cursor, block.end);
  }
  if (cursor + 45 > 1020) return null;
  const pad = (n) => String(n).padStart(2, "0");
  return pad(Math.floor(cursor / 60)) + ":" + pad(cursor % 60);
}
