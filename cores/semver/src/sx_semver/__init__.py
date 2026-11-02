from __future__ import annotations

import re

from sx.registry import Assurance, Core, Evidence, Example, Function
from sx.types import INT, STRING, OptionType, RecordType
from sx.values import NONE_VALUE, RecordValue, SomeValue, Value

VERSION = RecordType((("major", INT), ("minor", INT), ("patch", INT)), name="semver.Version")
PATTERN = re.compile(r"(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)")


def parts(value: Value) -> tuple[int, int, int]:
    assert isinstance(value, RecordValue)
    major, minor, patch = value.get("major"), value.get("minor"), value.get("patch")
    assert isinstance(major, int)
    assert isinstance(minor, int)
    assert isinstance(patch, int)
    return major, minor, patch


def parse(text: Value) -> Value:
    assert isinstance(text, str)
    found = PATTERN.fullmatch(text)
    if found is None:
        return NONE_VALUE
    major, minor, patch = (int(part) for part in found.groups())
    return SomeValue(RecordValue((("major", major), ("minor", minor), ("patch", patch))))


def compare(a: Value, b: Value) -> Value:
    x, y = parts(a), parts(b)
    return (x > y) - (x < y)


CORE = Core(
    id="semver",
    version="0.1.0",
    types={"semver.Version": VERSION},
    functions=(
        Function(
            "semver.parse", (STRING,), OptionType(VERSION),
            "Reads MAJOR.MINOR.PATCH, or none if the string is not exactly that.",
            Example('(semver.parse "1.4.2")', "(some {:major 1 :minor 4 :patch 2})"),
            parse,
        ),
        Function(
            "semver.compare", (VERSION, VERSION), INT,
            "-1, 0, or 1 as the first version is lower, equal, or higher.",
            Example("(semver.compare {:major 1 :minor 0 :patch 0} {:major 0 :minor 9 :patch 9})", "1"),
            compare,
        ),
        Function(
            "semver.show", (VERSION,), STRING,
            "Writes a version as MAJOR.MINOR.PATCH.",
            Example("(semver.show {:major 2 :minor 0 :patch 1})", '"2.0.1"'),
            lambda v: "{}.{}.{}".format(*parts(v)),
        ),
    ),
    evidence=(
        Evidence(
            Assurance.CONFORMANCE_TESTED,
            (
                "cores/semver/tests/test_semver_core.py::test_compare_orders_versions_by_number_not_by_text",
                "cores/semver/tests/test_semver_core.py::test_every_example_produces_its_documented_result",
            ),
        ),
    ),
)
