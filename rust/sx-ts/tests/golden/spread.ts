const base = { a: 1, b: 2 };
const merged = { ...base, c: 3, ...base };
const list = [...[1, 2], 3, ...[4]];
const { a, b: renamed } = base;
const [x, y] = [1, 2];
return { merged, list, a, renamed, x, y };
