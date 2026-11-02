export default async function main(api) {
  const hours = new Map();
  let uncovered = 0;
  let cursor = null;
  for (let page = 0; page < 10; page++) {
    const result = await api.oncall.shifts({ cursor });
    for (const shift of result.shifts) {
      hours.set(shift.person, (hours.get(shift.person) ?? 0) + shift.end - shift.start);
      if (shift.backup === null) uncovered++;
    }
    if (result.next === null) break;
    cursor = result.next;
  }
  const sorted = [...hours].map(([person, h]) => ({ person, hours: h })).sort((a, b) => b.hours - a.hours || (a.person < b.person ? -1 : 1));
  return { hours: sorted, uncovered };
}
