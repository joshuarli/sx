export default async function main(api) {
  const date = "2026-05-01";
  const rooms = (await api.rooms.search({ date })).sort((a, b) => a.cents - b.cents || (a.room < b.room ? -1 : 1));
  const tried = [];
  for (const { room } of rooms.slice(0, 3)) {
    tried.push(room);
    try {
      const { booking } = await api.rooms.book({ room, date, guest: "Ines Moreau" });
      return { booking, tried, error: null };
    } catch (error) {
      if (error.kind !== "Conflict") return { booking: null, tried, error: error.kind };
    }
  }
  return { booking: null, tried, error: "Conflict" };
}
