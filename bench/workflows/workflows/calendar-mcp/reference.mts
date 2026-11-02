type Calendar = { person: string; from: number; to: number; busy: [number, number][] };

export default async function main(api: Api) {
  const day = "2026-03-02";
  const people = ["ana", "ben", "chen", "dara"];
  const reads = await Promise.all(people.map((person) => api.mcpCalendar.getAvailability({ person, day })));
  const calendars: Calendar[] = reads.map((a) => ({
    person: a.person,
    from: a.working_hours?.start ?? 540,
    to: a.working_hours?.end ?? 1020,
    busy: a.busy.map((b): [number, number] => [b.start, b.end]),
  }));
  const plan = [
    { title: "Planning", attendees: ["ana", "ben", "chen"], length: 60 },
    { title: "Design review", attendees: ["ben", "chen", "dara"], length: 45 },
    { title: "Retro", attendees: ["ana", "ben"], length: 30 },
  ];
  const candidates = (attendees: string[], length: number, after: number) => {
    const starts: number[] = [];
    for (let start = 0; start + length <= 24 * 60; start += 15) {
      if (start <= after) continue;
      const everyoneFree = calendars
        .filter((c) => attendees.includes(c.person))
        .every((c) => start >= c.from && start + length <= c.to && c.busy.every(([b, e]) => !(start < e && b < start + length)));
      if (everyoneFree) starts.push(start);
    }
    return starts;
  };
  const meetings = [];
  for (const { title, attendees, length } of plan) {
    let after = -1;
    for (let refused = 0; ; refused++) {
      const start = candidates(attendees, length, after)[0]!;
      const end = start + length;
      try {
        const { meeting } = await api.mcpCalendar.bookMeeting({ title, day, start, end, attendees });
        for (const c of calendars) if (attendees.includes(c.person)) c.busy.push([start, end]);
        meetings.push({ title, id: meeting.id, start, end, room: meeting.room ?? null, refused });
        break;
      } catch (error) {
        if ((error as CallError).kind !== "Failed" || refused === 2) throw error;
        after = start;
      }
    }
  }
  return { meetings };
}
