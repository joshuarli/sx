export default async function main(api) {
  const members = await api.iam.members({ group: "finance" });
  const revoked = [];
  const failed = [];
  for (const member of members.filter((m) => m.idle > 60 && m.role !== "admin")) {
    try {
      await api.iam.revoke({ group: "finance", user: member.user });
      revoked.push(member.user);
    } catch (error) {
      if (error.kind === "Conflict") revoked.push(member.user);
      else failed.push({ user: member.user, kind: error.kind });
    }
  }
  return { revoked, failed };
}
