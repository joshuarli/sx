from __future__ import annotations

from collections.abc import Mapping

from sx.checker import DEFAULT_BUDGETS, Budgets, CheckResult, check
from sx.diagnostics import Diagnostic
from sx.reader import read
from sx.registry import Registry
from sx.syntax import Position, Span
from sx.types import Type


def compile_source(
    source: str,
    registry: Registry,
    *,
    allowed: frozenset[str],
    inputs: Mapping[str, Type] | None = None,
    budgets: Budgets = DEFAULT_BUDGETS,
) -> CheckResult:
    size = len(source.encode())
    if size > budgets.source_bytes:
        start = Position(1, 1)
        message = f"The program is {size} bytes, more than the source-bytes budget of {budgets.source_bytes}."
        return CheckResult(None, (Diagnostic(code="too-large", span=Span(start, start), message=message),))
    result = read(source)
    if result.program is None:
        return CheckResult(None, result.diagnostics)
    return check(result.program, registry, allowed=allowed, inputs=inputs, budgets=budgets)
