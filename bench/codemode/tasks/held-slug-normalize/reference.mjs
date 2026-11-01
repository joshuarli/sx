export default async function main(api) {
  const pages = await api.cms.pages();
  const seen = new Map();
  const updated = [];
  for (const page of pages) {
    const base = api.text.replace(api.text.casefold(api.text.trim(page.title)), " ", "-");
    const n = (seen.get(base) ?? 0) + 1;
    seen.set(base, n);
    const slug = n === 1 ? base : base + "-" + api.text.fromInt(n);
    if (page.slug !== slug) {
      await api.cms.setSlug({ id: page.id, slug });
      updated.push({ id: page.id, slug });
    }
  }
  return updated;
}
