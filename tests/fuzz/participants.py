from __future__ import annotations

from collections.abc import Iterable
from enum import StrEnum


class Participant(StrEnum):
    RUST = "rust"
    LEAN = "lean"
    PYTHON = "python"


FROZEN_KERNEL = "frozen-kernel"

SUPPORTED_BY: dict[str, frozenset[Participant]] = {
    FROZEN_KERNEL: frozenset(Participant),
}


def participants(features: Iterable[str]) -> frozenset[Participant]:
    found = frozenset(Participant)
    for feature in features:
        found &= SUPPORTED_BY[feature]
    return found
