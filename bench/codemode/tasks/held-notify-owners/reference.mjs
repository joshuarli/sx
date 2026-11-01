export default async function main(api) {
  const tasks = await api.tasks.overdue();
  const byOwner = new Map();
  let unowned = 0;
  for (const task of tasks) {
    if (task.owner === null) {
      unowned += 1;
    } else {
      if (!byOwner.has(task.owner)) byOwner.set(task.owner, []);
      byOwner.get(task.owner).push(task.title);
    }
  }
  const sent = [];
  const failed = [];
  for (const owner of [...byOwner.keys()].sort()) {
    const titles = byOwner.get(owner).sort();
    const input = { to: owner, text: titles.length + " overdue: " + titles.join("; ") };
    try {
      let result;
      try {
        result = await api.messaging.send(input);
      } catch (error) {
        if (error.kind !== "RateLimited") throw error;
        await api.sleep(1000);
        result = await api.messaging.send(input);
      }
      sent.push({ owner, message: result.message });
    } catch (error) {
      failed.push(owner);
    }
  }
  return { sent, failed, unowned };
}
