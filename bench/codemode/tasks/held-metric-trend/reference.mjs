export default async function main(api) {
  const names = await api.metrics.names();
  const all = await Promise.all(names.map((name) => api.metrics.series({ name })));
  return names
    .filter((name, i) => {
      const s = all[i];
      if (s.length < 3) return false;
      const [a, b, c] = s.slice(-3);
      return a < b && b < c && c >= 90;
    })
    .sort();
}
