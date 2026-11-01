export default async function main(api) {
  const names = await api.messaging.channels();
  const results = await Promise.all(
    names.map(async (name) => {
      try {
        const messages = await api.messaging.history({ channel: name });
        const mentions = messages.filter((msg) => api.text.contains(api.text.casefold(msg.text), "@oncall")).length;
        return { name, mentions };
      } catch (error) {
        return { name, mentions: null };
      }
    }),
  );
  return {
    channels: results
      .filter((r) => r.mentions !== null && r.mentions > 0)
      .sort((a, b) => b.mentions - a.mentions || (a.name < b.name ? -1 : 1)),
    unreadable: results.filter((r) => r.mentions === null).map((r) => r.name).sort(),
  };
}
