let count = 0;
let tag = "none";
const files = await tools.fs.list({ path: "." });
if (files.length > 3) {
  count = 3;
  tag = "many";
} else if (files.length > 0) {
  count += 1;
}
for (let i = 0; i < 5; i++) {
  if (i === 3) break;
  count++;
}
const flag = files.length > 1 && count > 0;
const pick = tag || "fallback";
const value = files.length ? files[0].name : "";
console.log(count, tag, flag, pick);
return { count, tag, flag, pick, value, kind: typeof count, first: files[0], last: files[files.length - 1] };
