import { readFileSync } from "node:fs";
import { classify, rewriteApi } from "../subset/classify.mjs";

const [corpus] = process.argv.slice(2);
for (const line of readFileSync(corpus, "utf8").split("\n")) {
  if (!line) continue;
  const { code, ...about } = JSON.parse(line);
  const verdict = classify(code);
  console.log(JSON.stringify({ ...about, rewritten: rewriteApi(code), parsed: verdict.parsed, callsTools: verdict.callsTools ?? false, accepted: verdict.tiers?.accepted ?? false, codes: Object.keys(verdict.codes ?? {}), calls: (verdict.calls ?? []).map((c) => c.name) }));
}
