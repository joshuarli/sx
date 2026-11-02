use sx_kernel::json::{Json, parse, python_equal};
use sx_kernel::registry::Registry;
use sx_packs::mcp::{arguments, filled};

fn json(text: &str) -> Json {
    parse(text).expect("valid JSON")
}

fn type_of(text: &str) -> sx_kernel::types::Type {
    Registry::new().parse(text).expect("valid type")
}

#[test]
fn a_none_record_field_is_omitted_at_every_depth_but_a_none_element_is_kept() {
    let ty = type_of("Record {rows: List<Record {id: Int, note: Option<String>}>, spots: List<Option<Int>>, top: Option<String>}");
    let input = json(r#"{"rows": [{"id": 1, "note": null}, {"id": 2, "note": "x"}], "spots": [1, null], "top": null}"#);
    let sent = arguments(&input, &ty);
    let expected = json(r#"{"rows": [{"id": 1}, {"id": 2, "note": "x"}], "spots": [1, null]}"#);
    assert!(python_equal(&sent, &expected), "{sent:?}");
}

#[test]
fn an_absent_optional_field_is_read_as_none_at_every_depth() {
    let ty = type_of("Record open {rows: List<Record open {id: Int, note: Option<String>}>, by: Map<String, Record {n: Option<Int>}>}");
    let output = json(r#"{"rows": [{"id": 1}], "by": {"k": {}}, "extra": true}"#);
    let read = filled(&output, &ty);
    let expected = json(r#"{"rows": [{"id": 1, "note": null}], "by": {"k": {"n": null}}, "extra": true}"#);
    assert!(python_equal(&read, &expected), "{read:?}");
}

#[test]
fn a_required_property_is_never_filled_in() {
    let ty = type_of("Record open {id: Int}");
    let read = filled(&json("{}"), &ty);
    assert!(python_equal(&read, &json("{}")), "{read:?}");
}
