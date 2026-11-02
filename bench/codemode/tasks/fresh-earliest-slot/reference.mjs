export default async function main(api) {
  const busy = [];
  for (const person of ["ana", "ben", "cho"]) busy.push(...(await api.calendar.busy({ person, day: "2031-03-14" })));
  for (let start = 540; start + 30 <= 1020; start += 30) {
    if (!busy.some((b) => start < b.to && b.from < start + 30)) return start;
  }
  return null;
}
