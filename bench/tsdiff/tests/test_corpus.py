from __future__ import annotations

from pathlib import Path

from tsdiff.corpus import Catalogue, Nested, Script, decode_result, pi_name, responses_of
from tsdiff.optionals import with_optional_nulls

TYPES = {"fs.Entry": "Record {name: String, kind: String}"}


def envelope(**fields: object) -> dict[str, object]:
    return {"content": [{"type": "text", "text": "{}"}], "isError": False, **fields}


def test_a_nested_result_is_unwrapped_to_the_declared_output() -> None:
    inner = envelope(structuredContent={"result": [1, 2]})
    result = {"content": [], "structuredContent": inner, "isError": False}
    assert decode_result(result, "List<Int>", "", TYPES) == ("ok", [1, 2])
    record = envelope(structuredContent={"name": "a", "kind": "File"})
    assert decode_result(record, "fs.Entry", "", TYPES) == ("ok", {"name": "a", "kind": "File"})
    assert decode_result(envelope(structuredContent={"result": 3}), "Int", "wrapped", TYPES) == ("ok", 3)


def test_a_text_result_is_the_joined_text() -> None:
    result = {"content": [{"type": "text", "text": "a"}, {"type": "text", "text": "b"}], "isError": False}
    assert decode_result(result, "String", "text", TYPES) == ("ok", "ab")


def test_an_error_result_keeps_its_kind_and_message_through_the_double_wrapping() -> None:
    inner = {"content": [], "structuredContent": {"kind": "PermissionDenied", "message": "no"}, "isError": True}
    result = {"content": [], "structuredContent": inner, "isError": True}
    assert decode_result(result, "String", "", TYPES) == ("error", {"kind": "PermissionDenied", "message": "no"})
    plain = {"content": [{"type": "text", "text": "boom"}], "isError": True}
    assert decode_result(plain, "String", "", TYPES) == ("error", {"kind": "Failed", "message": "boom"})
    assert decode_result(None, "String", "", TYPES)[0] == "missing"


def test_optional_input_fields_are_written_as_null_as_sx_encodes_them() -> None:
    assert with_optional_nulls({"q": "a"}, "Record {q: String, limit: Option<Int>}", {}) == {"q": "a", "limit": None}
    given = {"q": "a", "limit": 2}
    assert with_optional_nulls(given, "Record {q: String, limit: Option<Int>}", {}) == given
    assert with_optional_nulls({}, "Record {}", {}) == {}


def test_repeated_calls_with_one_input_become_one_entry_with_results_in_call_order() -> None:
    catalogue = Catalogue(
        declarations={}, grants={}, tools=[], outputs={"a.get": "Int"},
        inputs={"a.get": "Record {id: String, n: Option<Int>}"}, types={},
        names={pi_name("tools", "a.get"): ("a.get", "")}, wrapped={},
    )
    name = pi_name("tools", "a.get")
    failed = {"content": [], "structuredContent": {"kind": "Timeout", "message": "t"}, "isError": True}
    ok = envelope(structuredContent={"result": 7})
    script = Script("s", "16", "r", "t", "js-0", 0, "", Path("."), [
        Nested(name, {"id": "x"}, failed), Nested(name, {"id": "x"}, ok), Nested(name, {"id": "y"}, None),
    ])
    responses, why = responses_of(script, catalogue)
    assert why == ""
    results = [{"error": {"kind": "Timeout", "message": "t"}}, {"output": 7}]
    assert responses == [{"capability": "a.get", "input": {"id": "x", "n": None}, "results": results}]
    unknown = Script("s", "16", "r", "t", "js-0", 0, "", Path("."), [Nested("mcp__tools__zzz", {}, ok)])
    assert responses_of(unknown, catalogue)[0] is None
