from __future__ import annotations

from collections.abc import Sequence
from dataclasses import dataclass

from sx.syntax import Span

REPORTED_LIMIT = 20


@dataclass(frozen=True, slots=True)
class Diagnostic:
    code: str
    span: Span
    message: str
    construct: str | None = None
    detail: str | None = None
    fix: str | None = None
    help: str | None = None

    @property
    def label(self) -> str:
        return self.code if self.construct is None else f"{self.code}:{self.construct}"


def render(source: str, diagnostics: Sequence[Diagnostic]) -> str:
    lines = source.split("\n")
    blocks = [_render_one(lines, diagnostic) for diagnostic in diagnostics[:REPORTED_LIMIT]]
    hidden = len(diagnostics) - REPORTED_LIMIT
    if hidden > 0:
        blocks.append(f"{hidden} more error{'s' if hidden != 1 else ''} not shown.\n")
    return "\n".join(blocks)


def _render_one(lines: list[str], diagnostic: Diagnostic) -> str:
    span = diagnostic.span
    out = [f"error[{diagnostic.label}] {span}"]
    line_text = lines[span.start.line - 1] if span.start.line - 1 < len(lines) else ""
    start = span.start.column - 1
    end = span.end.column - 1 if span.end.line == span.start.line else len(line_text)
    width = max(1, end - start)
    out.append(f"  {line_text}")
    out.append("  " + " " * start + "^" * width)
    out.append(f"  {diagnostic.message}")
    if diagnostic.detail is not None:
        out.extend(f"  {detail_line}" for detail_line in diagnostic.detail.split("\n"))
    if diagnostic.fix is not None:
        out.append(f"  Fix: {diagnostic.fix}")
    if diagnostic.help is not None:
        out.append(f"  Help: sx help {diagnostic.help}")
    return "\n".join(out) + "\n"
