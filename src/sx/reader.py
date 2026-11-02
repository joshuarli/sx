from __future__ import annotations

import math
import re
from dataclasses import dataclass, field

from sx.diagnostics import Diagnostic
from sx.syntax import (
    DURATION_UNITS,
    BoolLiteral,
    DurationLiteral,
    FloatLiteral,
    Form,
    IntLiteral,
    Keyword,
    ListLiteral,
    Node,
    Position,
    RecordField,
    RecordLiteral,
    Span,
    StringLiteral,
    Symbol,
)

INT_MIN = -(2**63)
INT_MAX = 2**63 - 1

_WHITESPACE = frozenset(" \t\r\n,")
_DELIMITERS = frozenset("()[]{}\";")
_CLOSERS = {"(": ")", "[": "]", "{": "}"}

_INT = re.compile(r"-?[0-9]+")
_DURATION = re.compile(r"([0-9]+)(ms|s|m|h)")
_DURATION_LIKE = re.compile(r"(-?[0-9]+(?:\.[0-9]+)?)([a-z]+)")
_UNIT_SPELLINGS = {
    "msec": "ms", "millis": "ms", "sec": "s", "secs": "s", "second": "s", "seconds": "s",
    "min": "m", "mins": "m", "minute": "m", "minutes": "m", "hr": "h", "hrs": "h", "hour": "h", "hours": "h",
}
_UNIT_SIZES = dict(DURATION_UNITS)
_FLOAT = re.compile(r"-?[0-9]+(\.[0-9]+([eE][-+]?[0-9]+)?|[eE][-+]?[0-9]+)")
_UNFINISHED_FLOAT = re.compile(r"-?[0-9]+\.([eE][-+]?[0-9]+)?")
_NAME = r"[a-z][A-Za-z0-9_-]*(\?|'+)?"
_VARIANT = r"[A-Z][A-Za-z0-9]*"
_PLAIN = re.compile(_NAME)
_QUALIFIED = re.compile(rf"[a-z][a-z0-9]*(-[a-z0-9]+)*\.({_NAME}|{_VARIANT})")
_VARIANT_NAME = re.compile(_VARIANT)
_KEYWORD = re.compile(r":[A-Za-z_][A-Za-z0-9_-]*\??")
_OPERATORS = frozenset({"+", "-", "*", "/", "=", "not=", "<", "<=", ">", ">=", "_"})
_NIL_SPELLINGS = frozenset({"nil", "null", "undefined", "None"})
_JS_OPERATORS = {
    "&&": "and",
    "||": "or",
    "!": "not",
    "!=": "not=",
    "!==": "not=",
    "==": "=",
    "===": "=",
    "%": "rem",
}


def _indent(line: str) -> int:
    return len(line) - len(line.lstrip(" \t"))


def _is_blank(line: str) -> bool:
    text = line.strip()
    return not text or text.startswith(";")


@dataclass(frozen=True, slots=True)
class ReadResult:
    program: Node | None
    diagnostics: tuple[Diagnostic, ...]


@dataclass(frozen=True, slots=True)
class _Closed:
    opener: str
    opener_span: Span
    closer: str
    closer_span: Span


@dataclass(slots=True)
class _Frame:
    opener: str
    closer: str
    span: Span
    as_list: bool
    items: list[Node] = field(default_factory=list)
    poisoned: bool = False
    shorthand: bool = False


class _Reader:
    def __init__(self, source: str) -> None:
        self.source = source
        self.index = 0
        self.line = 1
        self.column = 1
        self.diagnostics: list[Diagnostic] = []
        self.stack: list[_Frame] = []
        self.top: list[Node] = []
        self.top_poisoned = False
        self.lines = source.split("\n")
        self.closed_top: list[_Closed] = []

    def position(self) -> Position:
        return Position(self.line, self.column)

    def peek(self, offset: int = 0) -> str:
        at = self.index + offset
        return self.source[at] if at < len(self.source) else ""

    def advance(self) -> str:
        char = self.source[self.index]
        self.index += 1
        if char == "\n":
            self.line += 1
            self.column = 1
        else:
            self.column += 1
        return char

    def error(self, code: str, span: Span, message: str, **extra: str | None) -> None:
        self.diagnostics.append(Diagnostic(code=code, span=span, message=message, **extra))
        self.poison()

    def poison(self) -> None:
        if self.stack:
            self.stack[-1].poisoned = True
        else:
            self.top_poisoned = True

    def emit(self, node: Node) -> None:
        if self.stack:
            self.stack[-1].items.append(node)
        else:
            self.top.append(node)

    def read(self) -> ReadResult:
        while self.index < len(self.source):
            char = self.peek()
            if char in _WHITESPACE:
                self.advance()
            elif char == ";":
                while self.index < len(self.source) and self.peek() != "\n":
                    self.advance()
            elif char in _CLOSERS:
                start = self.position()
                self.advance()
                self.open(char, _CLOSERS[char], Span(start, self.position()), as_list=char == "[")
            elif char in ")]}":
                self.close()
            elif char == '"':
                self.string()
            elif char == "#":
                self.dispatch()
            elif char == "'":
                start = self.position()
                self.advance()
                self.error(
                    "habit",
                    Span(start, self.position()),
                    "Sx has no quoting. Write a list literal as [a b c].",
                    construct="quote",
                )
            elif char in "`~@^":
                start = self.position()
                self.advance()
                self.error(
                    "habit",
                    Span(start, self.position()),
                    f"Sx has no reader macros, so {char} has no meaning.",
                    construct="reader-macro",
                )
            else:
                self.atom()
        for frame in self.stack:
            last, cut = self.form_end(frame.span)
            where = "at the end of the program" if last is None else f"at the end of line {last}"
            self.diagnostics.append(
                Diagnostic(
                    code="unclosed",
                    span=frame.span,
                    message=f"This {frame.opener} is never closed.",
                    detail=(
                        None if last is None
                        else f"Line {cut} is indented like a new form, not inside this {frame.opener}."
                    ),
                    fix=f"add {frame.closer} {where}",
                )
            )
        return self.finish()

    def form_end(self, opener: Span) -> tuple[int | None, int | None]:
        column = opener.start.column - 1
        last = opener.start.line
        for number in range(opener.start.line + 1, len(self.lines) + 1):
            text = self.lines[number - 1]
            if _is_blank(text):
                continue
            if _indent(text) <= column:
                return last, number
            last = number
        return None, None

    def finish(self) -> ReadResult:
        if not self.diagnostics:
            if not self.top:
                at = self.position()
                self.diagnostics.append(
                    Diagnostic(
                        code="empty-program",
                        span=Span(at, at),
                        message="The program is empty. A program is one expression.",
                    )
                )
            elif len(self.top) > 1:
                second = self.top[1]
                self.diagnostics.append(
                    Diagnostic(
                        code="multiple-expressions",
                        span=second.span,
                        message=f"A program is one expression, but this one has {len(self.top)}.",
                        detail="Bind intermediate steps with let and end with the result.",
                        fix="(let [a ... b ...] result)",
                        help="let",
                    )
                )
        if self.diagnostics:
            return ReadResult(None, tuple(self.diagnostics))
        return ReadResult(self.top[0], ())

    def open(self, opener: str, closer: str, span: Span, *, as_list: bool) -> None:
        self.stack.append(_Frame(opener=opener, closer=closer, span=span, as_list=as_list))

    def close(self) -> None:
        start = self.position()
        char = self.advance()
        self.close_with(char, Span(start, self.position()))

    def close_with(self, char: str, span: Span) -> None:
        if not self.stack:
            self.extra_close(char, span)
            return
        frame = self.stack[-1]
        if char != frame.closer and self.stays_open(frame, span):
            self.error(
                "unexpected-close", span,
                f"This {char} is one too many: the {frame.opener} at {_at(frame.span)} is still open, "
                f"because the next line is indented inside it.",
                fix="remove it",
            )
            return
        self.stack.pop()
        whole = Span(frame.span.start, span.end)
        resume = False
        if char != frame.closer:
            diagnostic, resume = self.mismatched_close(char, span, frame)
            self.diagnostics.append(diagnostic)
            frame.poisoned = True
        elif not self.stack:
            self.closed_top.append(_Closed(frame.opener, frame.span, char, span))
        node = self.build(frame, whole)
        if frame.poisoned:
            self.poison()
        if node is not None:
            self.emit(node)
        if resume:
            self.close_with(char, span)

    def stays_open(self, frame: _Frame, closer: Span) -> bool:
        rest = self.lines[closer.start.line - 1][closer.end.column - 1 :]
        if rest.strip(" \t)]}") and not rest.strip(" \t)]}").startswith(";"):
            return False
        column = frame.span.start.column - 1
        for text in self.lines[closer.start.line :]:
            if not _is_blank(text):
                return _indent(text) > column
        return False

    def extra_close(self, char: str, span: Span) -> None:
        message = f"This {char} closes nothing, because every bracket before it is already closed."
        closed = next((c for c in self.closed_top if self.encloses_by_indent(c, span)), None)
        if closed is None:
            self.error("unexpected-close", span, message, fix="remove it")
            return
        early = _at(closed.closer_span)
        self.error(
            "unexpected-close", span, message,
            detail=(
                f"The {closed.opener} at {_at(closed.opener_span)} closed at {early}, "
                f"but line {span.start.line} is indented as if inside it."
            ),
            fix=f"remove the {closed.closer} at {early}, then keep this one",
        )

    def encloses_by_indent(self, closed: _Closed, extra: Span) -> bool:
        if extra.start.line <= closed.closer_span.start.line:
            return False
        column = closed.opener_span.start.column - 1
        lines_between = self.lines[closed.closer_span.start.line : extra.start.line]
        return all(_is_blank(text) or _indent(text) > column for text in lines_between)

    def mismatched_close(self, char: str, span: Span, frame: _Frame) -> tuple[Diagnostic, bool]:
        message = f"Expected {frame.closer} to close the {frame.opener} at {_at(frame.span)}, found {char}."
        owner = next((f for f in reversed(self.stack) if f.closer == char), None)
        last, cut = self.form_end(frame.span)
        if owner is not None and last is not None and cut is not None and cut <= span.start.line:
            diagnostic = Diagnostic(
                code="unexpected-close",
                span=span,
                message=message,
                detail=(
                    f"This {char} most likely closes the {owner.opener} at {_at(owner.span)}. "
                    f"Line {cut} is indented like a new form, so the {frame.opener} at {_at(frame.span)} "
                    f"should end on line {last}."
                ),
                fix=f"add {frame.closer} at the end of line {last}",
            )
            return diagnostic, True
        diagnostic = Diagnostic(
            code="unexpected-close", span=span, message=message, fix=f"replace {char} with {frame.closer}",
        )
        return diagnostic, False

    def build(self, frame: _Frame, span: Span) -> Node | None:
        if frame.opener == "(":
            return Form(tuple(frame.items), span)
        if frame.as_list:
            return ListLiteral(tuple(frame.items), span)
        if frame.poisoned:
            return None
        return self.record(frame.items, span)

    def record(self, items: list[Node], span: Span) -> Node | None:
        fields: list[RecordField] = []
        seen: dict[str, Keyword] = {}
        ok = True
        for at in range(0, len(items), 2):
            key = items[at]
            if not isinstance(key, Keyword):
                ok = False
                fix = f":{key.value}" if isinstance(key, StringLiteral) else None
                self.diagnostics.append(
                    Diagnostic(
                        code="record-key",
                        span=key.span,
                        message="A record field name must be a keyword such as :name.",
                        fix=fix,
                    )
                )
                continue
            if at + 1 >= len(items):
                ok = False
                self.diagnostics.append(
                    Diagnostic(
                        code="record-missing-value",
                        span=key.span,
                        message=f"The field :{key.name} has no value.",
                        detail="A record literal alternates field names and values: {:a 1 :b 2}.",
                    )
                )
                continue
            if key.name in seen:
                ok = False
                self.diagnostics.append(
                    Diagnostic(
                        code="duplicate-field",
                        span=key.span,
                        message=f"The field :{key.name} appears twice in this record.",
                        detail=f"First use: {_at(seen[key.name].span)}",
                    )
                )
                continue
            seen[key.name] = key
            fields.append(RecordField(key, items[at + 1]))
        if not ok:
            self.poison()
            return None
        return RecordLiteral(tuple(fields), span)

    def dispatch(self) -> None:
        start = self.position()
        self.advance()
        nxt = self.peek()
        if nxt == "(":
            self.advance()
            span = Span(start, self.position())
            self.diagnostics.append(
                Diagnostic(
                    code="habit",
                    construct="anonymous-fn-shorthand",
                    span=span,
                    message="Sx has no #(...) shorthand. Write the function with fn and named parameters.",
                    fix="(fn [x] ...)",
                    help="fn",
                )
            )
            self.open("(", ")", span, as_list=False)
            self.stack[-1].poisoned = True
            self.stack[-1].shorthand = True
        elif nxt == "{":
            self.advance()
            span = Span(start, self.position())
            self.diagnostics.append(
                Diagnostic(
                    code="habit",
                    construct="set-literal",
                    span=span,
                    message="Sx has no set literals. Use a list, and distinct-by to remove duplicates.",
                    fix="[a b c]",
                    help="distinct-by",
                )
            )
            self.open("#{", "}", span, as_list=True)
            self.stack[-1].poisoned = True
        else:
            self.error("bad-literal", Span(start, self.position()), "Sx has no # syntax.")

    def string(self) -> None:
        start = self.position()
        self.advance()
        chars: list[str] = []
        while True:
            if self.index >= len(self.source):
                quote = Span(start, Position(start.line, start.column + 1))
                self.error("unclosed", quote, "This string is never closed.")
                return
            char = self.advance()
            if char == '"':
                self.emit(StringLiteral("".join(chars), Span(start, self.position())))
                return
            if char != "\\":
                chars.append(char)
                continue
            escape_start = Position(self.line, self.column - 1)
            escaped = self.advance() if self.index < len(self.source) else ""
            if escaped in {'"', "\\"}:
                chars.append(escaped)
            elif escaped == "n":
                chars.append("\n")
            elif escaped == "t":
                chars.append("\t")
            elif escaped == "r":
                chars.append("\r")
            elif escaped == "u" and self.peek() == "{":
                decoded = self.unicode_escape(escape_start)
                if decoded is not None:
                    chars.append(decoded)
            else:
                self.error(
                    "bad-escape",
                    Span(escape_start, self.position()),
                    f"\\{escaped} is not a string escape.",
                    detail='Escapes: \\" \\\\ \\n \\r \\t \\u{HEX}',
                )

    def unicode_escape(self, escape_start: Position) -> str | None:
        self.advance()
        digits: list[str] = []
        while self.index < len(self.source) and self.peek() not in '}"':
            digits.append(self.advance())
        if self.peek() == "}":
            self.advance()
        text = "".join(digits)
        span = Span(escape_start, self.position())
        if not re.fullmatch(r"[0-9A-Fa-f]{1,6}", text):
            self.error("bad-escape", span, f"\\u{{{text}}} needs 1 to 6 hexadecimal digits.")
            return None
        point = int(text, 16)
        if point > 0x10FFFF or 0xD800 <= point <= 0xDFFF:
            self.error("bad-escape", span, f"\\u{{{text}}} is not a Unicode scalar value.")
            return None
        return chr(point)

    def atom(self) -> None:
        start = self.position()
        chars: list[str] = []
        while self.index < len(self.source) and self.peek() not in _WHITESPACE and self.peek() not in _DELIMITERS:
            chars.append(self.advance())
        text = "".join(chars)
        span = Span(start, self.position())
        node = self.classify(text, span)
        if node is not None:
            self.emit(node)

    def classify(self, text: str, span: Span) -> Node | None:
        if text.startswith("%") and any(frame.shorthand for frame in self.stack):
            return None
        if _INT.fullmatch(text):
            value = int(text)
            if not INT_MIN <= value <= INT_MAX:
                self.error("int-range", span, f"{text} does not fit in a signed 64-bit Int.")
                return None
            return IntLiteral(value, span)
        duration = _DURATION.fullmatch(text)
        if duration:
            milliseconds = int(duration.group(1)) * _UNIT_SIZES[duration.group(2)]
            if milliseconds > INT_MAX:
                self.error("int-range", span, f"{text} is longer than the largest duration.")
                return None
            return DurationLiteral(milliseconds, span)
        near = _DURATION_LIKE.fullmatch(text)
        if near:
            self.bad_duration(near.group(1), near.group(2), span)
            return None
        if _FLOAT.fullmatch(text):
            number = float(text)
            if not math.isfinite(number):
                self.error("float-range", span, f"{text} does not fit in a finite 64-bit Float.")
                return None
            return FloatLiteral(number, span)
        if _UNFINISHED_FLOAT.fullmatch(text):
            self.error(
                "bad-literal", span, f"{text} is not a number. A Float has digits after the point, as in 1.0.",
                fix=text.replace(".", ".0", 1),
            )
            return None
        if text[0].isdigit() or (text[0] == "-" and text[1:2].isdigit()):
            self.error("bad-literal", span, f"{text} is not a number. An Int is written in decimal digits.")
            return None
        if text in {"true", "false"}:
            return BoolLiteral(text == "true", span)
        if text in {"True", "False"}:
            self.error("habit", span, "Booleans are lowercase.", construct="capitalized-boolean", fix=text.lower())
            return None
        if text in _NIL_SPELLINGS:
            self.error(
                "habit",
                span,
                f"Sx has no {text}. A missing value is none, of type Option.",
                construct="nil",
                fix="none",
                help="Option",
            )
            return None
        if text.startswith(":"):
            if _KEYWORD.fullmatch(text):
                return Keyword(text[1:], span)
            suggestion = _kebab(text[1:])
            fix = f":{suggestion}" if suggestion and _KEYWORD.fullmatch(f":{suggestion}") else None
            self.error(
                "bad-literal",
                span,
                f"{text} is not a valid keyword. A keyword is : and a field name, such as :repo-id.",
                fix=fix,
            )
            return None
        if text.endswith(":") and len(text) > 1:
            name = _kebab(text[:-1].strip("'\""))
            self.error(
                "habit",
                span,
                "This looks like a JavaScript object key. Record fields are keywords followed by a value.",
                construct="js-object-key",
                fix=f"{{:{name} value}}",
            )
            return None
        if text in _OPERATORS or _PLAIN.fullmatch(text) or _QUALIFIED.fullmatch(text) or _VARIANT_NAME.fullmatch(text):
            return Symbol(text, span)
        self.bad_symbol(text, span)
        return None

    def bad_duration(self, amount: str, unit: str, span: Span) -> None:
        unit = _UNIT_SPELLINGS.get(unit, unit)
        detail = "Durations are a whole number followed by ms, s, m, or h, for example 250ms or 5s."
        if unit not in _UNIT_SIZES:
            self.error("bad-literal", span, f"{amount}{unit} is not a duration.", detail=detail)
            return
        if amount.startswith("-"):
            self.error("bad-literal", span, "A duration cannot be negative.", detail=detail)
            return
        milliseconds = float(amount) * _UNIT_SIZES[unit]
        fix = f"{int(milliseconds)}ms" if milliseconds.is_integer() else None
        if "." not in amount:
            fix = f"{amount}{unit}"
        self.error("bad-literal", span, "This is not a duration literal.", detail=detail, fix=fix)

    def bad_symbol(self, text: str, span: Span) -> None:
        if text in {"->", "->>", "some->", "as->", "cond->"}:
            self.error(
                "habit",
                span,
                f"Sx has no threading macros such as {text}. Nest the calls, or bind each step with let.",
                construct="threading",
                help="let",
            )
        elif text in _JS_OPERATORS:
            self.error(
                "habit",
                span,
                f"Sx has no {text} operator.",
                construct="js-operator",
                fix=_JS_OPERATORS[text],
            )
        elif text.startswith("."):
            self.error(
                "habit",
                span,
                "Sx has no methods. Call a function with the value as an argument.",
                construct="method-call",
            )
        elif "/" in text:
            self.error(
                "habit",
                span,
                "Sx qualifies names with a dot, as in text.join, not with a slash.",
                construct="namespace-slash",
            )
        else:
            suggestion = _kebab(text)
            valid = bool(suggestion) and bool(_PLAIN.fullmatch(suggestion) or _QUALIFIED.fullmatch(suggestion))
            self.error(
                "bad-symbol",
                span,
                f"{text} is not a valid name. A name starts with a lowercase letter, then letters, digits, - and _.",
                fix=suggestion if valid else None,
            )


def _kebab(text: str) -> str:
    spaced = re.sub(r"(?<=[a-z0-9])([A-Z])", r"-\1", text)
    return spaced.replace("_", "-").lower()


def _at(span: Span) -> str:
    return f"{span.start.line}:{span.start.column}"


def read(source: str) -> ReadResult:
    return _Reader(source).read()
