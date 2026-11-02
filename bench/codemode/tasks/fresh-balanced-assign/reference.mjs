export default async function main(api) {
  const loads = await api.desk.workloads();
  const assignments = [];
  for (const ticket of await api.desk.unassigned()) {
    loads.sort((a, b) => a.count - b.count || (a.agent < b.agent ? -1 : 1));
    const agent = loads[0];
    await api.desk.assign({ ticket, agent: agent.agent });
    agent.count++;
    assignments.push({ ticket, agent: agent.agent });
  }
  return assignments;
}
