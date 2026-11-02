const out = [];
for (let i = 0; i < 3; i++) {
  out.push(i * 2);
}
for (let j = 10; j >= 0; j -= 5) {
  out.push(j);
}
for (let k = 1; k <= 4; k++) {
  out.push(await tools.fs.readText({ path: `f${k}` }));
}
return out;
