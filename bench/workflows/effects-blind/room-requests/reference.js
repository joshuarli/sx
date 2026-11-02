const READ = tools.mcp__tools__fs_read_text;
const BOOKINGS = tools.mcp__tools__rooms_bookings;
const BOOK = tools.mcp__tools__rooms_book;
const CANCEL = tools.mcp__tools__rooms_cancel;

const raw = (await READ({ path: "{{root}}/requests/rooms.txt" })).content[0].text;
const lines = raw.split("\n").filter((line) => line.trim() !== "");
const existing = (await BOOKINGS({})).structuredContent.result;

const outcomes = [];
for (const line of lines) {
  const fields = line.split(" | ")[0].split(" ");
  if (fields[0] === "BOOK") {
    const answer = await BOOK({ room: fields[1], start: fields[2], end: fields[3], owner: fields[4] });
    outcomes.push(answer.isError ? "skipped" : "booked");
  } else if (fields[0] === "CANCEL" && existing.some((b) => b.id === fields[1] && b.owner === fields[2])) {
    const answer = await CANCEL({ id: fields[1] });
    outcomes.push(answer.isError ? "skipped" : "cancelled");
  } else {
    outcomes.push("skipped");
  }
}
return outcomes;
