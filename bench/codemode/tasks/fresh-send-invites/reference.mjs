async function invite(api, person) {
  for (let attempt = 0; ; attempt++) {
    try {
      await api.calendar.invite({ meeting: "m-77", person });
      return true;
    } catch (error) {
      if (error.kind !== "Unavailable" || attempt === 2) return false;
      await api.sleep(1000);
    }
  }
}

export default async function main(api) {
  const invited = [];
  const rejected = [];
  for (const person of ["ana", "ben", "cho", "dev"]) (await invite(api, person) ? invited : rejected).push(person);
  return { invited, rejected };
}
