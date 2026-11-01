export default async function main(api) {
  const tickets = (await api.tickets.listOpen())
    .filter((t) => t.priority === "P1" && t.assignee === null)
    .sort((a, b) => a.id - b.id);
  const teams = [...new Set(tickets.map((t) => t.team))];
  const owners = new Map();
  await Promise.all(
    teams.map(async (team) => {
      try {
        owners.set(team, (await api.oncall.current({ team })).user);
      } catch (error) {
        owners.set(team, null);
      }
    }),
  );
  const assigned = [];
  const unassigned = [];
  for (const ticket of tickets) {
    const user = owners.get(ticket.team);
    if (user === null) {
      unassigned.push(ticket.id);
    } else {
      await api.tickets.assign({ id: ticket.id, user });
      assigned.push({ ticket: ticket.id, user });
    }
  }
  return { assigned, unassigned };
}
