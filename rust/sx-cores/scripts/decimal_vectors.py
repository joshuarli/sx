import json
import re
from decimal import (
    ROUND_CEILING,
    ROUND_DOWN,
    ROUND_FLOOR,
    ROUND_HALF_EVEN,
    ROUND_HALF_UP,
    ROUND_UP,
    Context,
    Decimal,
    DivisionByZero,
    InvalidOperation,
)
from pathlib import Path

OUTPUT = Path(__file__).resolve().parents[3] / "fixtures" / "cores"
VERSION = "0.1.0"
MAX_DIGITS = 38
MAX_SCALE = 38
MASK = 2**64 - 1
INT_MAX = 2**63 - 1
INT_MIN = -(2**63)
LITERAL = re.compile(r"-?[0-9]+(\.[0-9]+)?")
MODES = {
    "HalfEven": ROUND_HALF_EVEN,
    "HalfUp": ROUND_HALF_UP,
    "Down": ROUND_DOWN,
    "Up": ROUND_UP,
    "Floor": ROUND_FLOOR,
    "Ceiling": ROUND_CEILING,
}
WIDE = Context(prec=400, Emax=10**6, Emin=-(10**6))

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

    def pick[T](self, items: list[T]) -> T:
        return items[self.below(len(items))]


def canonical(value: Decimal) -> str | None:
    sign, digits, exponent = value.as_tuple()
    assert isinstance(exponent, int)
    coefficient = int("".join(map(str, digits)) or "0")
    if coefficient == 0:
        return "0"
    scale = max(-exponent, 0)
    coefficient *= 10 ** max(exponent, 0)
    while scale > 0 and coefficient % 10 == 0:
        coefficient //= 10
        scale -= 1
    if len(str(coefficient)) > MAX_DIGITS or scale > MAX_SCALE:
        return None
    text = str(coefficient).rjust(scale + 1, "0")
    if scale:
        text = f"{text[:-scale]}.{text[-scale:]}"
    return f"-{text}" if sign else text


def parse(text: str) -> Decimal | None:
    if LITERAL.fullmatch(text) is None:
        return None
    value = Decimal(text)
    return value if canonical(value) is not None else None


def must(text: str) -> Decimal:
    value = parse(text)
    assert value is not None
    return value


class Vectors:
    def __init__(self) -> None:
        self.items: list[dict[str, Json]] = []
        self.seen: set[str] = set()

    def add(self, name: str, args: list[Json], result: Json = None, *, fails: bool = False) -> None:
        key = json.dumps([name, args])
        if key in self.seen:
            return
        self.seen.add(key)
        item: dict[str, Json] = {"function": name, "args": args}
        if fails:
            item["fails"] = "arithmetic"
        else:
            item["result"] = result
        self.items.append(item)

    def unary(self, name: str, text: str) -> None:
        value = must(text)
        shown = canonical(value)
        if name == "decimal.show":
            self.add(name, [text], shown)
        else:
            assert name == "decimal.to-float"
            self.add(name, [text], float(value))

    def parse(self, text: str) -> None:
        value = parse(text)
        self.add("decimal.parse", [text], None if value is None else canonical(value))

    def from_int(self, n: int) -> None:
        self.add("decimal.from-int", [n], canonical(Decimal(n)))

    def arithmetic(self, name: str, left: str, right: str) -> None:
        a, b = must(left), must(right)
        operation = {"decimal.add": WIDE.add, "decimal.sub": WIDE.subtract, "decimal.mul": WIDE.multiply}[name]
        shown = canonical(operation(a, b))
        self.add(name, [left, right], shown, fails=shown is None)

    def compare(self, left: str, right: str) -> None:
        a, b = must(left), must(right)
        self.add("decimal.compare", [left, right], (a > b) - (a < b))

    def divide(self, left: str, right: str, places: int, mode: str) -> None:
        a, b = must(left), must(right)
        args: list[Json] = [left, right, places, mode]
        if not 0 <= places <= MAX_SCALE:
            self.add("decimal.div", args, fails=True)
            return
        try:
            quotient = WIDE.divide(a, b)
        except (DivisionByZero, InvalidOperation):
            self.add("decimal.div", args, fails=True)
            return
        shown = canonical(quotient.quantize(Decimal(1).scaleb(-places), rounding=MODES[mode], context=WIDE))
        self.add("decimal.div", args, shown, fails=shown is None)

    def round(self, text: str, places: int, mode: str) -> None:
        value = must(text)
        args: list[Json] = [text, places, mode]
        if not 0 <= places <= MAX_SCALE:
            self.add("decimal.round", args, fails=True)
            return
        rounded = value.quantize(Decimal(1).scaleb(-places), rounding=MODES[mode], context=WIDE)
        shown = canonical(rounded)
        self.add("decimal.round", args, shown, fails=shown is None)

    def format(self, text: str, places: int) -> None:
        value = must(text)
        args: list[Json] = [text, places]
        if not 0 <= places <= MAX_SCALE:
            self.add("decimal.format", args, fails=True)
            return
        rounded = value.quantize(Decimal(1).scaleb(-places), rounding=ROUND_HALF_EVEN, context=WIDE)
        if canonical(rounded) is None:
            self.add("decimal.format", args, fails=True)
            return
        shown = format(rounded, "f")
        self.add("decimal.format", args, shown.removeprefix("-") if rounded == 0 else shown)

    def render(self) -> str:
        lines = ",\n".join(json.dumps(item, separators=(",", ":")) for item in self.items)
        return f'{{"core":"decimal","version":"{VERSION}","vectors":[\n{lines}\n]}}\n'


NINES = "9" * MAX_DIGITS

LITERALS = [
    "0", "-0", "0.0", "-0.0", "00", "007", "7", "-7", "1.5", "1.50", "1.500000", "-1.50", "0.5", "0.05", "0.005",
    "12.50", "-12.50", "100", "100.00", "1000000", "0.1", "0.2", "0.3", "1.005", "2.675", "2.5", "3.5", "-2.5",
    "-3.5", "0.125", "0.375", "0.0625", "99.995", "-99.995", "123456789.123456789", "1e3", "1E3", "+1", "1.", ".5",
    "", "-", "--1", "1.2.3", " 1", "1 ", "1_000", "1,5", "NaN", "Infinity", "-Infinity", "0x10", "١٢٣", "1e-2",
    NINES, "-" + NINES, NINES + "0", "1" + "0" * 37, "1" + "0" * 38, "0." + "0" * 37 + "1", "0." + "0" * 38 + "1",
    "0." + "9" * 38, "0." + "9" * 39, "1." + "0" * 60, "1" + "0" * 37 + ".0", "12345678901234567890.12345678901234567",
    "12345678901234567890.123456789012345678", "-0." + "0" * 37 + "5", "0.00000001", "00000.5000",
    "9" * 19 + "." + "9" * 19, "9" * 20 + "." + "9" * 19, "5" * 37, "5" * 37 + "5", "-" + "5" * 38,
    "9223372036854775807", "-9223372036854775808", "9223372036854775808",
]
VALID = [text for text in LITERALS if parse(text) is not None]
SMALL = [
    "0", "1", "-1", "0.5", "-0.5", "1.5", "-1.5", "2.5", "-2.5", "3.5", "0.25", "-0.25", "0.125", "10", "0.1", "0.3",
    "7", "-7", "1.005", "2.675", "99.995", "-123.456", "0.045", "1234.5678",
]


def random_literal(rng: Rng) -> str:
    while True:
        text = draw_literal(rng)
        if parse(text) is not None:
            return text


def draw_literal(rng: Rng) -> str:
    whole = rng.between(1, 30) if rng.below(4) else rng.between(1, 38)
    fraction = rng.between(0, 38 - min(whole, 37)) if rng.below(3) else rng.between(0, 12)
    digits = "".join(str(rng.below(10)) for _ in range(whole + fraction))
    if rng.below(3) == 0:
        digits = digits[: whole + fraction - 1] + "0"
    text = digits[:whole].lstrip("0") or "0"
    if fraction:
        text += "." + digits[whole:]
    return ("-" if rng.below(3) == 0 else "") + text


def build() -> Vectors:
    vectors = Vectors()
    for text in LITERALS:
        vectors.parse(text)
    for text in VALID:
        vectors.unary("decimal.show", text)
        vectors.unary("decimal.to-float", text)
    for n in (0, 1, -1, 7, 42, -42, 10**9, INT_MAX, INT_MIN, INT_MAX - 1, INT_MIN + 1):
        vectors.from_int(n)
    for left in VALID:
        for right in ("0", "1", "-1", "0.5", "99.995", NINES, "0." + "9" * 38):
            for name in ("decimal.add", "decimal.sub", "decimal.mul"):
                vectors.arithmetic(name, left, right)
            vectors.compare(left, right)
    for left in SMALL:
        for right in SMALL:
            for name in ("decimal.add", "decimal.sub", "decimal.mul"):
                vectors.arithmetic(name, left, right)
            vectors.compare(left, right)
    for left in SMALL:
        for right in ("0", "1", "-1", "3", "-3", "0.3", "12.5"):
            for places in (0, 2):
                for mode in MODES:
                    vectors.divide(left, right, places, mode)
    for left in VALID:
        for right in ("1", "3", "-7", "0.3", "0", NINES, "0." + "9" * 38):
            for places in (0, 2, 38):
                vectors.divide(left, right, places, "HalfEven")
    for places in (-1, 39, 100, INT_MAX, INT_MIN):
        vectors.divide("1", "3", places, "HalfEven")
        vectors.round("1.5", places, "HalfEven")
        vectors.format("1.5", places)
    for text in SMALL + VALID:
        for places in (0, 1, 2, 38):
            for mode in MODES:
                vectors.round(text, places, mode)
            vectors.format(text, places)
    rng = Rng(20260501)
    for _ in range(300):
        left, right = random_literal(rng), random_literal(rng)
        for name in ("decimal.add", "decimal.sub", "decimal.mul"):
            vectors.arithmetic(name, left, right)
        vectors.compare(left, right)
        vectors.divide(left, right, rng.between(0, 38), rng.pick(list(MODES)))
        vectors.round(left, rng.between(0, 38), rng.pick(list(MODES)))
        vectors.format(left, rng.between(0, 38))
        vectors.unary("decimal.show", left)
        vectors.unary("decimal.to-float", left)
        vectors.parse(left)
    return vectors


def render() -> str:
    return build().render()


def main() -> None:
    OUTPUT.mkdir(parents=True, exist_ok=True)
    (OUTPUT / "decimal.json").write_text(render(), encoding="utf-8")


if __name__ == "__main__":
    main()
