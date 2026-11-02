const ALERTS = tools.mcp__tools__alerts_list;
const MESSAGES = tools.mcp__tools__chat_messages;
const POST = tools.mcp__tools__chat_post;

const alerts = (await ALERTS({})).structuredContent.result;
const newest = alerts
  .filter((a) => a.severity === "critical")
  .sort((a, b) => (a.raised_at < b.raised_at ? 1 : -1))
  .slice(0, 3);
const announced = [];
for (const alert of newest) {
  const request = { channel: "ops", text: `Critical alert ${alert.id} on ${alert.service}: ${alert.text}` };
  let answer = await POST(request);
  for (let wait = 1000; answer.isError && answer.structuredContent.kind === "RateLimited" && wait <= 2000; wait *= 2) {
    await new Promise((resolve) => setTimeout(resolve, wait));
    answer = await POST(request);
  }
  if (answer.isError) {
    const held = (await MESSAGES({ channel: "ops" })).structuredContent.result;
    if (!held.some((m) => m.text.includes(alert.id))) answer = await POST(request);
  }
  announced.push(alert.id);
}
return announced;
