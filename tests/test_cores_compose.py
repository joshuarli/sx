from sx_semver import CORE as SEMVER
from sx_text import CORE as TEXT

from sx.host import evaluate
from sx.registry import Registry
from sx.types import STRING, ListType
from sx.values import ListValue

LATEST = """
(let [clean (fn [tag]
              (let [lower (text.casefold (text.trim tag))]
                (if (text.starts-with? lower "v") (text.join "" (rest (text.chars lower))) lower)))
      versions (keep (fn [tag] (semver.parse (clean tag))) tags)
      ordered (sort-by (fn [v] [(get v :major) (get v :minor) (get v :patch)]) versions)]
  (if (= (count ordered) 0) "none" (semver.show (nth ordered (- (count ordered) 1)))))
"""


def latest(tags: list[str]) -> object:
    registry = Registry()
    registry.add_core(TEXT)
    registry.add_core(SEMVER)
    return evaluate(registry, LATEST, {"tags": (ListType(STRING), ListValue(tuple(tags)))})


def test_semver_composes_with_text_without_kernel_changes() -> None:
    assert latest(["V1.9.3", " v1.10.0 ", "nightly", "1.2.0"]) == "1.10.0"
    assert latest(["nightly", "1.2"]) == "none"
