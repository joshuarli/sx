const VERSION = /^(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)$/;

export const text = {
  casefold: (s) => s.toLowerCase(),
  split: (s, separator) => s.split(separator),
  join: (separator, parts) => parts.join(separator),
  chars: (s) => [...s],
  trim: (s) => s.trim(),
  length: (s) => [...s].length,
  startsWith: (s, prefix) => s.startsWith(prefix),
  endsWith: (s, suffix) => s.endsWith(suffix),
  contains: (s, part) => s.includes(part),
  replace: (s, from, to) => s.replaceAll(from, to),
  fromInt: (n) => String(n),
  toInt: (s) => (/^-?[0-9]+$/.test(s) ? Number(s) : null),
};

export const semver = {
  parse: (s) => {
    const match = VERSION.exec(s);
    return match ? { major: Number(match[1]), minor: Number(match[2]), patch: Number(match[3]) } : null;
  },
  compare: (a, b) => {
    for (const key of ["major", "minor", "patch"]) {
      if (a[key] !== b[key]) return a[key] < b[key] ? -1 : 1;
    }
    return 0;
  },
  show: (v) => `${v.major}.${v.minor}.${v.patch}`,
};
