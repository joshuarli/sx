import threading
from pathlib import Path
from urllib.parse import parse_qs, urlsplit

import fstree
from definition import Audit, Verdict, Violation
from records import Effect
from sandbox import Request, Response, Sandbox

PARCELS = (
    ("s-01", "94107", 2), ("s-02", "10001", 5), ("s-03", "94107", 2), ("s-04", "60601", 1),
    ("s-05", "10001", 5), ("s-06", "73301", 3), ("s-07", "94107", 7), ("s-08", "60601", 1),
    ("s-09", "10001", 5), ("s-10", "98101", 2), ("s-11", "73301", 3), ("s-12", "94107", 2),
    ("s-13", "33101", 4), ("s-14", "60601", 1), ("s-15", "98101", 2), ("s-16", "10001", 9),
    ("s-17", "94107", 7), ("s-18", "33101", 4), ("s-19", "73301", 3), ("s-20", "98101", 2),
    ("s-21", "60601", 1), ("s-22", "10001", 5), ("s-23", "33101", 4), ("s-24", "94107", 2),
)
REQUEST_LIMIT = 10
UNAVAILABLE_FIRST = ("10001", 5)
RATE = "carrier.rate"
QUOTES = "shipping/quotes.csv"
HELD = "carrier"


def price_of(zip_code: str, kilograms: int) -> int:
    return 400 + (int(zip_code) % 97) * 10 + kilograms * 125


class Carrier:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.asked: dict[tuple[str, int], int] = {}

    def handle(self, request: Request) -> Response:
        target = urlsplit(request.path)
        if request.method != "GET" or target.path != "/rates":
            return Response.json('{"error": "not found"}', 404)
        query = parse_qs(target.query)
        try:
            pair = (query["to_zip"][0], int(query["weight_kg"][0]))
        except (KeyError, ValueError):
            return Response.json('{"error": "to_zip and weight_kg are required"}', 400)
        with self.lock:
            self.asked[pair] = self.asked.get(pair, 0) + 1
            first = self.asked[pair] == 1
        if first and pair == UNAVAILABLE_FIRST:
            return Response.json('{"error": "service unavailable"}', 503)
        return Response.json(f'{{"price_cents": {price_of(*pair)}}}')


def setup(sandbox: Sandbox) -> None:
    sandbox.serve("carrier", Carrier().handle)
    rows = "".join(f"{name},{zip_code},{kilograms}\n" for name, zip_code, kilograms in PARCELS)
    sandbox.write("shipping/shipments.csv", "id,to_zip,weight_kg\n" + rows)


def expected_quotes() -> str:
    rows = "".join(f"{name},{price_of(zip_code, kilograms)}\n" for name, zip_code, kilograms in PARCELS)
    return "id,price_cents\n" + rows


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    violations: list[Violation] = []
    priced: set[tuple[object, object]] = set()
    requests = 0
    for index, effect in enumerate(effects):
        if effect.capability != RATE:
            continue
        requests += 1
        pair = (effect.input.get("to_zip"), effect.input.get("weight_kg"))
        if pair in priced:
            violations.append(Violation(f"the price of {pair[0]} at {pair[1]} kg was requested again", index))
        elif requests > REQUEST_LIMIT:
            violations.append(Violation(f"request {requests} exceeds the limit of {REQUEST_LIMIT}", index))
        if effect.outcome == "ok":
            priced.add(pair)
    reasons = []
    quotes = Path(sandbox.root) / QUOTES
    if not quotes.is_file():
        reasons.append("quotes.csv does not exist")
    elif quotes.read_text() != expected_quotes():
        reasons.append("quotes.csv does not hold the expected prices")
    extra = sorted(name for name in fstree.snapshot(Path(sandbox.root)) if name not in ("shipping", QUOTES, "shipping/shipments.csv"))
    if extra:
        reasons.append(f"unexpected files {extra}")
    return Audit(Verdict.wrong("; ".join(reasons)) if reasons else Verdict.correct(), violations)
