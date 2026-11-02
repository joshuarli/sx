import json
import sys
from collections.abc import Sequence
from itertools import product
from pathlib import Path

from sx_git import CORE as GIT
from sx_github import CORE as GITHUB
from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.registry import Core
from sx.values import decode, encode

OUTPUT = Path(__file__).resolve().parents[3] / "fixtures" / "cores"
SURROGATES = range(0xD800, 0xE000)
INT_MAX = 2**63 - 1
INT_MIN = -(2**63)
MASK = 2**64 - 1

type Json = object


class Rng:
    def __init__(self, seed: int) -> None:
        self.state = seed

    def next(self) -> int:
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def below(self, bound: int) -> int:
        return self.next() % bound

    def between(self, low: int, high: int) -> int:
        return low + self.below(high - low + 1)

    def pick[T](self, items: Sequence[T]) -> T:
        return items[self.below(len(items))]

    def chance(self, percent: int) -> bool:
        return self.below(100) < percent

    def string(self, alphabet: Sequence[str], low: int, high: int) -> str:
        return "".join(self.pick(alphabet) for _ in range(self.between(low, high)))


class Vectors:
    def __init__(self, core: Core) -> None:
        self.core = core
        self.functions = {function.name: function for function in core.functions}
        self.items: list[dict[str, Json]] = []
        self.seen: set[str] = set()

    def add(self, name: str, *args: Json) -> None:
        key = json.dumps([name, args])
        if key in self.seen:
            return
        self.seen.add(key)
        function = self.functions[name]
        assert function.implementation is not None
        values = [decode(arg, kind) for arg, kind in zip(args, function.params, strict=True)]
        item: dict[str, Json] = {"function": name, "args": list(args)}
        try:
            result = function.implementation(*values)
        except ValueError:
            item["fails"] = "ValueError"
        else:
            item["result"] = encode(result, function.output)
        self.items.append(item)

    def render(self) -> str:
        lines = ",\n".join(json.dumps(item, separators=(",", ":")) for item in self.items)
        return f'{{"core":"{self.core.id}","version":"{self.core.version}","vectors":[\n{lines}\n]}}\n'


def code_point(rng: Rng) -> str:
    low, high = rng.pick(
        (
            (0, 0x7F), (0x80, 0x24F), (0x370, 0x52F), (0x1E00, 0x1FFF), (0x2000, 0x2BFF),
            (0x3000, 0xD7FF), (0xE000, 0xFFFF), (0x10000, 0x1FFFF), (0x20000, 0x2FFFF), (0x100000, 0x10FFFF),
        )
    )
    return chr(rng.between(low, high))


def unicode_text(rng: Rng, low: int, high: int) -> str:
    return "".join(code_point(rng) for _ in range(rng.between(low, high)))


def integer(rng: Rng) -> int:
    magnitude = min(rng.next() >> rng.below(64), INT_MAX)
    return -magnitude if rng.chance(50) else magnitude


def code_points() -> list[str]:
    return [chr(cp) for cp in range(sys.maxunicode + 1) if cp not in SURROGATES]


EDGE_TEXT = (
    "", " ", "a", "A", "ab", "LaPuta", "Stra\u00dfe", "STRASSE", "straSSe", "\u0130", "i\u0307",
    "\u01c5", "\u01c6", "\u01c4", "\ufb01", "\u0149",
    "\u1e9e", "\u0391\u03a3", "\u03a7\u0391\u039f\u03a3", "\u1f88", "\u0345", "\u017f", "\u212a", "\u00c5",
    "\U0001f600",
    "e\u0301", "\u00e9", "\x00", "\x85", "\u2028",
    "\t\n a b \r\n", "a,b,,c", ",", ",,", "aaaa", "abcabc", "a\U0001f600b\U0001f600",
    "\ud7ff\ue000\uffff\U00010000\U0010ffff",
)
SPACE_LOOKALIKES = ("\u200b", "\u200c", "\u200d", "\u2060", "\ufeff", "\u180e", "\x00", "\x1b", "\x20\u0301")
SEPARATORS = ("", ",", "aa", "abc", "\U0001f600", "\u00e9", ",,", "x", " ")
SOURCES_FOR_AFFIX = (
    "", "a", "ab", "\u00df", "\U0001f600\u00e9", "v1.2", "a.md", "hello", "\u0130stanbul", "aaa", "e\u0301",
)


def text_vectors() -> Vectors:
    rng = Rng(0x7E47)
    vectors = Vectors(TEXT)
    points = code_points()
    folded = [c for c in points if c.casefold() != c]
    spaces = [c for c in points if c.isspace()]
    pool = (*EDGE_TEXT, *folded[::7], *spaces, *SPACE_LOOKALIKES, "a", "b", "Z")

    for s in EDGE_TEXT:
        vectors.add("text.casefold", s)
    for c in folded:
        vectors.add("text.casefold", c)
    for start in range(0, len(folded), 24):
        vectors.add("text.casefold", "".join(folded[start : start + 24]))
    for _ in range(600):
        vectors.add("text.casefold", rng.string(pool, 1, 10))
    for _ in range(300):
        vectors.add("text.casefold", unicode_text(rng, 0, 12))
    for c in points[:: len(points) // 400]:
        vectors.add("text.casefold", c)

    for s, separator in product(EDGE_TEXT, SEPARATORS):
        vectors.add("text.split", s, separator)
    for _ in range(300):
        vectors.add("text.split", rng.string("ab, ", 0, 12), rng.pick((",", " ", "a", "ab", ",,", ", ", "b")))
    for _ in range(100):
        vectors.add("text.split", unicode_text(rng, 0, 8), rng.pick(SEPARATORS))

    part_lists = (
        [], [""], ["a"], ["a", "b"], ["", ""], ["\u00df", "\u0130", "x"], ["a", "", "b", ""],
        ["\U0001f600", "\u00e9"],
    )
    for separator, parts in product(("", ",", ", ", "\U0001f600", "ab"), part_lists):
        vectors.add("text.join", separator, parts)
    for _ in range(150):
        vectors.add(
            "text.join", rng.pick(SEPARATORS), [rng.string("ab,\u00e9", 0, 4) for _ in range(rng.between(0, 5))]
        )

    for s in EDGE_TEXT:
        vectors.add("text.chars", s)
    for _ in range(100):
        vectors.add("text.chars", unicode_text(rng, 0, 10))

    for s in EDGE_TEXT:
        vectors.add("text.trim", s)
        vectors.add("text.length", s)
    for c in (*spaces, *SPACE_LOOKALIKES):
        for template in ("{0}", "{0}a{0}", "{0}{0}a b{0}", "a{0}", "{0}a", " {0} "):
            vectors.add("text.trim", template.format(c))
    for _ in range(300):
        vectors.add("text.trim", rng.string((*spaces, *SPACE_LOOKALIKES, "a", "b", " "), 0, 10))
    for _ in range(200):
        s = unicode_text(rng, 0, 20)
        vectors.add("text.trim", s)
        vectors.add("text.length", s)

    for name in ("text.starts-with?", "text.ends-with?", "text.contains?"):
        for s, other in product(SOURCES_FOR_AFFIX, SOURCES_FOR_AFFIX):
            vectors.add(name, s, other)
        for _ in range(200):
            s = rng.string("ab\U0001f600\u00e9", 0, 8)
            cut_low, cut_high = sorted((rng.between(0, len(s)), rng.between(0, len(s))))
            vectors.add(name, s, rng.pick((s[:cut_high], s[cut_low:], s[cut_low:cut_high], rng.string("ab", 0, 3))))

    for s, old, new in product(
        ("", "a-b-c", "aaaa", "abab", "\u00df", "\U0001f600x\U0001f600", "e\u0301"),
        ("", "-", "a", "aa", "ab", "\U0001f600", "\u00e9"),
        ("", "+", "aa", "-"),
    ):
        vectors.add("text.replace", s, old, new)
    for _ in range(300):
        vectors.add("text.replace", rng.string("ab", 0, 10), rng.string("ab", 0, 3), rng.string("abc", 0, 3))

    for n in (0, 1, -1, 9, 10, -10, 99, 100, 2**31, -(2**31), 2**53, -(2**53), 2**53 + 1, INT_MAX, INT_MIN):
        vectors.add("text.from-int", n)
    for _ in range(100):
        vectors.add("text.from-int", integer(rng))

    to_int_edges = (
        "", "-", "--1", "+1", "1", "-1", "0", "-0", "00", "007", "-007", "1_000", "1.0", "1e3", " 1", "1 ", "1\n",
        "\n1", "\uff11\uff12", "\u0663", "-\u0663", "0x10", "9223372036854775807",
        "9223372036854775808", "-9223372036854775808",
        "-9223372036854775809", "00009223372036854775807", "99999999999999999999", "1" * 19, "1" * 20, "0" * 400 + "7",
        "1" * 4000, "-" + "9" * 4000, "abc", "1-", "- 1",
    )
    for s in to_int_edges:
        vectors.add("text.to-int", s)
    for n in (0, 42, INT_MAX, INT_MIN):
        vectors.add("text.to-int", str(n))
    for _ in range(300):
        digits = rng.string("0123456789", 1, 22)
        vectors.add("text.to-int", rng.pick(("", "-", "-", "+")) + rng.pick(("", "0", "00")) + digits)
    for _ in range(150):
        vectors.add("text.to-int", rng.string("0123456789-+ .\u0663\uff11e", 0, 8))
    for _ in range(100):
        vectors.add("text.to-int", str(integer(rng)))
    return vectors


def version(major: int, minor: int, patch: int) -> dict[str, int]:
    return {"major": major, "minor": minor, "patch": patch}


def semver_vectors() -> Vectors:
    rng = Rng(0x5E3F)
    vectors = Vectors(SEMVER)
    for s in (
        "1.4.2", "0.0.0", "10.20.30", "1.2", "1.2.3.4", "01.2.3", "1.02.3", "1.2.03", "v1.2.3", "1.2.-3", " 1.2.3",
        "1.2.3 ", "1.2.3\n", "", ".", "..", "1..3", ".1.2", "1.2.", "1.2.3-rc1", "1.2.3+build", "1.2.x", "\uff11.2.3",
        "1.\u0663.3", "1,2,3", "+1.2.3", "9223372036854775807.0.0",
        "0.9223372036854775807.0", "0.0.9223372036854775807",
        "00.0.0", "0.00.0", "1.10.0", "1.9.9", "1" * 18 + ".1.1",
    ):
        vectors.add("semver.parse", s)
    for _ in range(400):
        parts = [str(rng.pick((0, 1, 2, 9, 10, 99, rng.between(0, 5000), rng.next() >> 1))) for _ in range(3)]
        vectors.add("semver.parse", ".".join(parts))
    mutations = (
        lambda p: p[:2], lambda p: [*p, "4"], lambda p: ["0" + p[0], *p[1:]], lambda p: [p[0], p[1], "-" + p[2]],
        lambda p: ["v" + p[0], *p[1:]], lambda p: [*p[:2], p[2] + "\n"], lambda p: [*p[:2], p[2] + "-x"],
        lambda p: [p[0], "", p[2]], lambda p: [p[0], p[1], " " + p[2]], lambda p: [p[0], p[1] + "a", p[2]],
    )
    for _ in range(300):
        parts = [str(rng.between(0, 30)) for _ in range(3)]
        vectors.add("semver.parse", ".".join(rng.pick(mutations)(parts)))
    for _ in range(150):
        vectors.add("semver.parse", rng.string("0123456789.v-+ \n", 0, 9))

    versions = [version(*parts) for parts in product((0, 1, 2), repeat=3)]
    extremes = [version(INT_MAX, INT_MAX, INT_MAX), version(INT_MIN, INT_MIN, INT_MIN), version(0, INT_MAX, 0)]
    for a, b in product([*versions, *extremes], [*versions[::4], *extremes]):
        vectors.add("semver.compare", a, b)
    for _ in range(300):
        pair = [version(*(rng.between(0, 12) for _ in range(3))) for _ in range(2)]
        vectors.add("semver.compare", *pair)
    for _ in range(100):
        vectors.add("semver.compare", version(*map(integer, [rng] * 3)), version(*map(integer, [rng] * 3)))
    for v in [*versions, *extremes, version(1, 10, 0), version(-1, 0, -5)]:
        vectors.add("semver.show", v)
    for _ in range(100):
        vectors.add("semver.show", version(*map(integer, [rng] * 3)))
    return vectors


def head(
    staged: int = 0, unstaged: int = 0, untracked: int = 0, ahead: int | None = None, behind: int | None = None,
    upstream: str | None = None,
) -> dict[str, Json]:
    return {
        "staged": staged, "unstaged": unstaged, "untracked": untracked, "ahead": ahead, "behind": behind,
        "upstream": upstream,
    }


def on_branch(branch: str, head_value: dict[str, Json]) -> Json:
    return {"tag": "OnBranch", "value": {"branch": branch, "head": head_value}}


def origin_mismatch(branch: str | None, head_value: dict[str, Json], error: str) -> Json:
    return {"tag": "OriginMismatch", "value": {"branch": branch, "head": head_value, "error": error}}


def random_head(rng: Rng) -> dict[str, Json]:
    count = (0, 0, 1, 2, 5, -1, INT_MAX, INT_MIN)
    optional = (None, None, 0, 1, 3, -2, INT_MAX)
    return head(
        rng.pick(count), rng.pick(count), rng.pick(count), rng.pick(optional), rng.pick(optional),
        rng.pick((None, "origin/main", "")),
    )


def random_checkout(rng: Rng) -> Json:
    kind = rng.below(5)
    branch = rng.pick(("main", "topic", "", "feature/\u00df", "\U0001f600"))
    if kind == 0:
        return "Missing"
    if kind == 1:
        return {"tag": "Invalid", "value": rng.pick(("not a checkout", "", "boom"))}
    if kind == 2:
        return origin_mismatch(rng.pick((None, branch)), random_head(rng), rng.pick(("wrong origin", "")))
    if kind == 3:
        return {"tag": "Detached", "value": random_head(rng)}
    return on_branch(branch, random_head(rng))


def git_vectors() -> Vectors:
    rng = Rng(0x61F7)
    vectors = Vectors(GIT)
    fixed: list[Json] = [
        "Missing",
        {"tag": "Invalid", "value": "not a checkout"},
        {"tag": "Invalid", "value": ""},
        origin_mismatch("main", head(), "wrong origin"),
        origin_mismatch(None, head(1), "x"),
        {"tag": "Detached", "value": head(untracked=2)},
        {"tag": "Detached", "value": head(ahead=1, behind=1)},
        on_branch("main", head()),
        on_branch("topic", head(upstream="origin/topic")),
    ]
    checkouts = [*fixed, *(random_checkout(rng) for _ in range(120))]
    for checkout in checkouts:
        vectors.add("git.head", checkout)
        vectors.add("git.branch", checkout)
        vectors.add("git.error", checkout)

    for checkout in fixed[:7]:
        for default, days, unpushed in product((None, "main"), (None, 3), (None, 9)):
            vectors.add("git.local-label", checkout, default, days, unpushed)
    counts = ((0, 0, 0), (1, 1, 1))
    sides = (None, 0, 2, -1)
    for (staged, unstaged, untracked), ahead, behind in product(counts, sides, sides):
        for branch, default, days, unpushed in product(
            ("main", "topic"), (None, "main", "topic"), (None, 3), (None, 0, 9)
        ):
            checkout = on_branch(branch, head(staged, unstaged, untracked, ahead, behind))
            vectors.add("git.local-label", checkout, default, days, unpushed)
    for _ in range(600):
        vectors.add(
            "git.local-label",
            random_checkout(rng),
            rng.pick((None, "main", "topic", "")),
            rng.pick((None, 0, 3, 7, -1, INT_MAX)),
            rng.pick((None, 0, 9, -3, INT_MIN)),
        )

    nows = (0, 1, 86_399_999, 86_400_000, 172_799_999, 172_800_000, 10**12, INT_MAX, -5, -86_400_000, INT_MIN)
    sinces = (None, 0, 1, -1, 5, 86_400, 172_800, 10**9, 9_223_372_036_854_775, INT_MAX, INT_MIN)
    for now, since in product(nows, sinces):
        vectors.add("git.age-days", now, since)
    for _ in range(200):
        vectors.add("git.age-days", integer(rng), rng.pick((None, integer(rng), rng.between(0, 2_000_000_000))))

    ages = (None, 0, 6, 7, 8, -1, 100, INT_MAX, INT_MIN)
    for a, b in product(ages, ages):
        vectors.add("git.stale-work?", a, b)
    return vectors


def check(
    name: str, state: str, source: str | None = None, updated_at: str | None = None, suite_id: str | None = None,
    run_id: str | None = None, attempt: int | None = None,
) -> dict[str, Json]:
    return {
        "name": name, "state": state, "source": source, "updated-at": updated_at, "suite-id": suite_id,
        "run-id": run_id, "attempt": attempt,
    }


ACTIONS = "GitHub Actions"
D20, D21, MINUTE_LATER = "2026-10-20T12:00:00Z", "2026-10-21T12:00:00Z", "2026-10-21T12:01:00Z"
FAILURES = ("failure", "failed", "error", "timed_out", "action_required", "startup_failure")
PENDINGS = ("queued", "requested", "waiting", "pending", "expected", "in_progress")
REPOMOD_CASES: tuple[tuple[list[dict[str, Json]], bool], ...] = (
    ([], True),
    ([], False),
    ([check("build", "failure")], True),
    ([check("old success", "success"), check("rerun", "failure")], True),
    ([check("build", "in_progress")], True),
    ([check("build", "cancelled")], True),
    ([check("workflow", "skipped")], True),
    ([check("external", "neutral")], True),
    ([check("build", "success")], True),
    (
        [check("build", "failure", ACTIONS, D20, "suite-a", "run-a", 1),
         check("build", "success", ACTIONS, D21, "suite-b", "run-a", 2)],
        True,
    ),
    (
        [check("build", "failure", ACTIONS, D20, "suite-a", None, 1),
         check("build", "cancelled", ACTIONS, D21, "suite-a", None, 2)],
        True,
    ),
    (
        [check("build", "failure", ACTIONS, D21, "suite-a", None, 1),
         check("build", "success", ACTIONS, MINUTE_LATER, "suite-b", None, 1)],
        True,
    ),
    (
        [check("build", "failure", ACTIONS, None, "suite-a", None, 1),
         check("build", "in_progress", ACTIONS, None, "suite-a", None, 2),
         check("deploy preview", "success", "status:preview", D21),
         check("security scan", "failure", "status:scanner", D21)],
        True,
    ),
    (
        [check("build", "failure", ACTIONS, None, "suite-a", None, 1),
         check("build", "in_progress", ACTIONS, None, "suite-a", None, 2)],
        True,
    ),
    (
        [check("deploy", "failure", "status:preview-a", D21),
         check("deploy", "success", "status:preview-b", MINUTE_LATER)],
        True,
    ),
    ([check("lint", "stale")], True),
    ([check("lint", "mystery")], True),
    ([check("lint", "success"), check("scan", "stale")], True),
    (
        [check("deploy", "failure", "status:preview", D20), check("deploy", "success", "status:preview", D21)],
        True,
    ),
    (
        [check("deploy", "success", "status:preview", D21), check("deploy", "failure", "status:preview", D21)],
        True,
    ),
    ([check("build", "success")], False),
)
NAMES = ("build", "Build", "deploy", "\u00df", "ss", "SS", "\u01c5", "\u01c6", "\u0130", "lint")
STATES = (
    *FAILURES, *PENDINGS, "cancelled", "success", "passed", "neutral", "skipped", "stale", "mystery", "FAILURE",
    "Success", "IN_PROGRESS", "\u017fuccess", "STALE", "Cancelled", "NEUTRAL", "Skipped", "", "\u0130N_PROGRESS",
)
COMMON_STATES = ("success", "success", "neutral", "skipped", "failure", "in_progress", "cancelled", "stale", "mystery")
STATUS_SOURCES = (
    "status:preview", "status:Preview", "status:scanner", "status:", "status:\u00df", "status:ss", "status:SS",
)
OTHER_SOURCES = (None, None, None, ACTIONS, "STATUS:preview", "Status:x", "external")
TIMESTAMPS = (None, D20, D21, MINUTE_LATER, "\uffff", "\U00010000", "", "z")
IDENTIFIERS = (None, None, "s1", "s2", "", "r1")
ATTEMPTS = (None, None, 0, 1, 2, 3, -1)


def random_check(rng: Rng, status_percent: int) -> dict[str, Json]:
    status = rng.chance(status_percent)
    return check(
        rng.pick(NAMES),
        rng.pick(COMMON_STATES if rng.chance(70) else STATES),
        rng.pick(STATUS_SOURCES if status else OTHER_SOURCES),
        rng.pick(TIMESTAMPS),
        rng.pick(IDENTIFIERS),
        rng.pick(IDENTIFIERS),
        rng.pick(ATTEMPTS),
    )


def colliding_status(rng: Rng) -> dict[str, Json]:
    return check(
        rng.pick(("deploy", "\u00df", "ss")),
        rng.pick(STATES),
        rng.pick(("status:preview", "status:PREVIEW")),
        rng.pick((D20, D21)),
        rng.pick(IDENTIFIERS),
        rng.pick(IDENTIFIERS),
        rng.pick(ATTEMPTS),
    )


def github_vectors() -> Vectors:
    rng = Rng(0x617B)
    vectors = Vectors(GITHUB)
    for checks, complete in REPOMOD_CASES:
        vectors.add("github.ci-state", checks, complete)
    for state in (*FAILURES, *PENDINGS, "cancelled", "success", "passed", "neutral", "skipped", "stale", "other"):
        vectors.add("github.ci-state", [check("a", state)], True)
        vectors.add("github.ci-state", [check("a", state.upper())], True)
        vectors.add("github.ci-state", [check("a", "success"), check("b", state)], True)
        vectors.add("github.ci-state", [check("a", "neutral"), check("b", state)], True)
        vectors.add("github.ci-state", [check("a", "skipped"), check("b", state)], True)
    for percent, count in ((15, 900), (80, 700)):
        for _ in range(count):
            checks = [random_check(rng, percent) for _ in range(rng.between(0, 6))]
            vectors.add("github.ci-state", checks, not rng.chance(8))

    for _ in range(500):
        vectors.add("github.ci-state", [colliding_status(rng) for _ in range(rng.between(2, 5))], True)

    for name in (
        "Passed", "Failed", "Pending", "Cancelled", "Skipped", "Neutral", "NoChecks", "Unknown", "Stale",
    ):
        vectors.add("github.ci-state-name", name)
    for label in (
        "passed", "failed", "pending", "cancelled", "skipped", "neutral", "none", "unknown", "stale", "PASSED", "",
        "success", "NoChecks", "nochecks", "passed ", " passed", "Passed", "failure", "\u017ftale", "p\u00e4ssed",
    ):
        vectors.add("github.parse-ci-state", label)
    for name in ("Identical", "Ahead", "Behind", "Diverged", "Unknown"):
        vectors.add("github.upstream-state-name", name)
    return vectors


BUILDERS = {"text": text_vectors, "semver": semver_vectors, "git": git_vectors, "github": github_vectors}


def render() -> dict[str, str]:
    return {core: build().render() for core, build in BUILDERS.items()}


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    for core, text in render().items():
        (OUTPUT / f"{core}.json").write_text(text, encoding="utf-8")


if __name__ == "__main__":
    main()
