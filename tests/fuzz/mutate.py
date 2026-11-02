from __future__ import annotations

import random
import re

TOKEN = re.compile(r'"(?:[^"\\]|\\.)*"|[()\[\]{}]|[^\s()\[\]{}"]+')
VOCABULARY = (
    "(", ")", "[", "]", "{", "}", "nil", "none", "_", "(some 1)", "(ok 1)", "(err 1)", "true", "false", "0", "-1",
    "9223372036854775808", "1e999", "1.5", "\"x\"", "\"\\q\"", ":else", ":x", "(recur)", "(recur 1)", "(fn [])",
    "(fn [x])", "(let [])", "(let [x])", "(cond)", "(match 1)", "(get)", "(call)", "(call nope.x {})", "5s", "0ms",
    "Failing", "Timeout", "(par)", "(par-map {:limit 0} identity [])", "(loop {:max 0} [] 1)", "(retry {} 1)",
    "(wait-until {} identity 1)", "(sleep 1)", "(sleep 5s 5s)", "(attempt)", "if", "let", "fn", "'x", "`x", "~x",
    "#(+ 1 %)", "@x", "x/y", "(.foo x)", "{:a}", "{:a 1 :a 2}", "#{1}", ";", "\\", "\"\\u{110000}\"",
)


def soup(rng: random.Random) -> str:
    pieces = [*VOCABULARY, "foo", "x", "1", "\"abc\"", "\n", "  ", ",", "; c\n", "é", "😀", "\u0000", "\ud7ff"]
    return " ".join(rng.choice(pieces) for _ in range(rng.randint(1, 30)))


def mutate(source: str, rng: random.Random) -> str:
    if rng.random() < 0.15:
        return soup(rng)
    for _ in range(rng.choice([1, 1, 2, 3])):
        source = one_mutation(source, rng)
    return source


def one_mutation(source: str, rng: random.Random) -> str:
    tokens: list[str] = [str(token) for token in TOKEN.findall(source)]
    if not tokens:
        return source + rng.choice(VOCABULARY)
    roll = rng.randrange(9)
    at = rng.randrange(len(tokens))
    if roll == 0:
        del tokens[at]
    elif roll == 1:
        tokens[at] = rng.choice(VOCABULARY)
    elif roll == 2:
        tokens.insert(at, rng.choice(VOCABULARY))
    elif roll == 3:
        other = rng.randrange(len(tokens))
        tokens[at], tokens[other] = tokens[other], tokens[at]
    elif roll == 4:
        tokens[at] = rng.choice(tokens)
    elif roll == 5:
        return source[: rng.randrange(len(source) + 1)]
    elif roll == 6:
        cut = rng.randrange(len(source))
        return source[:cut] + source[cut + rng.randint(1, 8) :]
    elif roll == 7:
        cut = rng.randrange(len(source) + 1)
        odd = rng.choice(["\x00", "\t", "\u00a0", "\u2028", "é", "\ufeff", "\ud7ff", "😀", "\\"])
        return source[:cut] + odd + source[cut:]
    else:
        tokens.insert(at, " ".join(tokens[at : at + rng.randint(1, 6)]))
    return " ".join(tokens)
