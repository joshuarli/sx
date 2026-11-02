import pytest
from typescript import convert


@pytest.mark.parametrize(
    "fields",
    ["{id: String, size: Float}", "{data: Bytes}", "{}"],
)
def test_an_open_record_declares_the_same_typescript_type_as_a_closed_one(fields: str) -> None:
    assert convert(f"Record open {fields}", {}) == convert(f"Record {fields}", {})


def test_float_is_a_number_and_bytes_is_a_base64_string() -> None:
    assert convert("Record {size: Float, data: Bytes}", {}) == "{ size: number; data: string }"
