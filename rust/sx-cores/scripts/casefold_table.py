import sys
from pathlib import Path

TARGET = Path(__file__).resolve().parent.parent / "src" / "tables.rs"
SURROGATES = range(0xD800, 0xE000)


def escape(text: str) -> str:
    return "".join(f"\\u{{{ord(c):x}}}" for c in text)


def render() -> str:
    code_points = [cp for cp in range(sys.maxunicode + 1) if cp not in SURROGATES]
    folds = [(cp, chr(cp).casefold()) for cp in code_points if chr(cp).casefold() != chr(cp)]
    spaces = [cp for cp in code_points if chr(cp).isspace()]
    lines = ["pub(crate) const CASEFOLD: &[(char, &str)] = &["]
    lines += [f"    ('\\u{{{cp:x}}}', \"{escape(folded)}\")," for cp, folded in folds]
    lines += ["];", "", "pub(crate) const STRIPPED_WHITESPACE: &[char] = &["]
    lines += [f"    '\\u{{{cp:x}}}'," for cp in spaces]
    lines += ["];", ""]
    return "\n".join(lines)


def main() -> None:
    TARGET.write_text(render(), encoding="utf-8")


if __name__ == "__main__":
    main()
