export default async function main(api) {
  const accounts = await api.ledger.accounts();
  const entries = await Promise.all(accounts.map((account) => api.ledger.entries({ account })));
  return accounts
    .map((account, i) => ({
      account,
      balance: entries[i].reduce((sum, e) => sum + (e.kind === "Credit" ? e.cents : -e.cents), 0),
    }))
    .sort((a, b) => a.balance - b.balance || (a.account < b.account ? -1 : 1));
}
