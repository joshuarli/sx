interface Pkg { name: string; version: string | null; deps: string[] }
const text = await tools.fs.readText({ path: "package.json" });
const pkg = JSON.parse(text) as Pkg;
const raw: { n: number; tags: Record<string, number> } = JSON.parse("{}");
return { pkg, raw };
