async function reply(api, ticket, text) {
  for (let attempt = 0; ; attempt++) {
    try {
      await api.desk.reply({ ticket, text });
      return true;
    } catch (error) {
      if (error.kind !== "RateLimited" || attempt === 2) return false;
      await api.sleep(5000);
    }
  }
}

export default async function main(api) {
  const sent = [];
  const failed = [];
  const replies = [["T-1", "Fixed in 2.3"], ["T-2", "Please send logs"], ["T-3", "Closing as duplicate"]];
  for (const [ticket, text] of replies) (await reply(api, ticket, text) ? sent : failed).push(ticket);
  return { sent, failed };
}
