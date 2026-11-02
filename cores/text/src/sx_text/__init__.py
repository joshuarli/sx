from __future__ import annotations

import re

from sx.registry import Assurance, Core, Evidence, Example, Function
from sx.types import BOOL, INT, STRING, ListType, OptionType
from sx.values import INT_MAX, INT_MIN, NONE_VALUE, ListValue, SomeValue, Value

DECIMAL = re.compile(r"-?[0-9]+")


def strings(value: Value) -> list[str]:
    assert isinstance(value, ListValue)
    return [item for item in value.items if isinstance(item, str)]


def to_int(text: Value) -> Value:
    assert isinstance(text, str)
    if DECIMAL.fullmatch(text) is None or not INT_MIN <= int(text) <= INT_MAX:
        return NONE_VALUE
    return SomeValue(int(text))


CORE = Core(
    id="text",
    version="0.1.0",
    functions=(
        Function(
            "text.casefold", (STRING,), STRING,
            "Lowercases for case-insensitive comparison, as Python str.casefold does.",
            Example('(text.casefold "LaPuta")', '"laputa"'),
            lambda s: s.casefold(),
        ),
        Function(
            "text.split", (STRING, STRING), ListType(STRING),
            "Splits the first string at every occurrence of the second.",
            Example('(text.split "a,b,,c" ",")', '["a" "b" "" "c"]'),
            lambda s, separator: ListValue(tuple(s.split(separator))),
        ),
        Function(
            "text.join", (STRING, ListType(STRING)), STRING,
            "Joins the strings with the separator between each pair.",
            Example('(text.join ", " ["a" "b"])', '"a, b"'),
            lambda separator, parts: separator.join(strings(parts)),
        ),
        Function(
            "text.chars", (STRING,), ListType(STRING),
            "Splits a string into one-character strings.",
            Example('(text.chars "ab")', '["a" "b"]'),
            lambda s: ListValue(tuple(s)),
        ),
        Function(
            "text.trim", (STRING,), STRING,
            "Removes leading and trailing whitespace.",
            Example('(text.trim "  a ")', '"a"'),
            lambda s: s.strip(),
        ),
        Function(
            "text.length", (STRING,), INT,
            "Counts characters.",
            Example('(text.length "abc")', "3"),
            lambda s: len(s),
        ),
        Function(
            "text.starts-with?", (STRING, STRING), BOOL,
            "True when the first string starts with the second.",
            Example('(text.starts-with? "v1.2" "v")', "true"),
            lambda s, start: s.startswith(start),
        ),
        Function(
            "text.ends-with?", (STRING, STRING), BOOL,
            "True when the first string ends with the second.",
            Example('(text.ends-with? "a.md" ".md")', "true"),
            lambda s, end: s.endswith(end),
        ),
        Function(
            "text.contains?", (STRING, STRING), BOOL,
            "True when the second string occurs in the first.",
            Example('(text.contains? "hello" "ell")', "true"),
            lambda s, part: part in s,
        ),
        Function(
            "text.replace", (STRING, STRING, STRING), STRING,
            "Replaces every occurrence of the second string with the third.",
            Example('(text.replace "a-b-c" "-" "+")', '"a+b+c"'),
            lambda s, old, new: s.replace(old, new),
        ),
        Function(
            "text.from-int", (INT,), STRING,
            "Writes an Int in decimal.",
            Example("(text.from-int -42)", '"-42"'),
            lambda n: str(n),
        ),
        Function(
            "text.to-int", (STRING,), OptionType(INT),
            "Reads a decimal Int, or none if the string is not one.",
            Example('(text.to-int "17")', "(some 17)"),
            to_int,
        ),
    ),
    evidence=(
        Evidence(
            Assurance.CONFORMANCE_TESTED,
            (
                "cores/text/tests/test_text_core.py::test_casefold_matches_python_for_every_code_point",
                "cores/text/tests/test_text_core.py::test_every_example_produces_its_documented_result",
            ),
        ),
    ),
)
