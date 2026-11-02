import unicodedata
from collections.abc import Callable
from pathlib import Path

TARGET = Path(__file__).resolve().parent.parent / "sx-kernel" / "src" / "unicode" / "tables.rs"


def ranges(predicate: Callable[[str], bool]) -> list[tuple[int, int]]:
    found: list[tuple[int, int]] = []
    start: int | None = None
    for point in range(0x110001):
        inside = point < 0x110000 and predicate(chr(point))
        if inside and start is None:
            start = point
        elif not inside and start is not None:
            found.append((start, point - 1))
            start = None
    return found


def table(name: str, predicate: Callable[[str], bool]) -> str:
    rows = "".join(f"    (0x{low:X}, 0x{high:X}),\n" for low, high in ranges(predicate))
    return f"pub const {name}: &[(u32, u32)] = &[\n{rows}];\n"


def main() -> int:
    text = (
        f'pub const UNICODE_VERSION: &str = "{unicodedata.unidata_version}";\n\n'
        + table("NOT_PRINTABLE", lambda c: not c.isprintable())
        + "\n"
        + table("DIGIT", str.isdigit)
        + "\n"
        + table("SPACE", str.isspace)
    )
    TARGET.write_text(text)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
