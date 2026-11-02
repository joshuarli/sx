#!/bin/sh
set -eu
cd "$(dirname "$0")"
command -v lake >/dev/null || { echo "axioms.sh: lake is not on PATH. Install elan and add ~/.elan/bin to PATH." >&2; exit 1; }
lake build
out=$(lake env lean Axioms.lean)
printf '%s\n' "$out"
expected=$(grep -c '^#print axioms' Axioms.lean)
found=$(printf '%s\n' "$out" | grep -c "depends on axioms\|does not depend on any axioms" || true)
if [ "$found" -ne "$expected" ]; then
  echo "axioms check: expected $expected theorems, found $found" >&2
  exit 1
fi
bad=$(printf '%s\n' "$out" | grep 'depends on axioms' | sed 's/.*depends on axioms: \[\(.*\)\]/\1/' | tr ',' '\n' \
  | sed 's/^ *//' | grep -v '^propext$' | grep -v '^Classical.choice$' | grep -v '^Quot.sound$' || true)
if [ -n "$bad" ]; then
  echo "axioms check: nonstandard axioms: $bad" >&2
  exit 1
fi
echo "axioms check: $expected theorems use only propext, Classical.choice, and Quot.sound"
