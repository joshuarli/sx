import json
import threading
from dataclasses import dataclass
from urllib.parse import parse_qs, urlsplit

from definition import Audit, AuditError, Verdict, Violation
from records import Effect
from sandbox import Request, Response, Sandbox

STOCK = (
    ("ws-100", "Hex bolt M8", 140, 100, 400),
    ("ws-220", "Washer 8mm", 35, 80, 300),
    ("ws-340", "Anchor 6x40", 12, 50, 200),
    ("ws-415", "Cable tie 200", 60, 150, 500),
    ("ws-530", "Gasket DN25", 8, 25, 100),
    ("ws-610", "Fuse 10A", 200, 200, 600),
    ("ws-725", "Hinge 90", 49, 50, 120),
)
SEEDED = (
    ("PO-9001", "ws-340", 200, "open"),
    ("PO-9002", "ws-725", 120, "received"),
)
COMMITS_THEN_UNAVAILABLE = frozenset({"ws-220"})
BAD_GATEWAY_WITHOUT_COMMIT = frozenset({"ws-415"})
RATE_LIMITED_FIRST = frozenset({"ws-530"})
ORDER = "warehouse.order"
HELD = "warehouse"


@dataclass(frozen=True, slots=True)
class Placed:
    id: str
    sku: str
    quantity: int
    status: str


@dataclass(frozen=True, slots=True)
class Post:
    sku: str
    quantity: int
    committed: bool


class Warehouse:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.stock = {sku: (name, on_hand, below, target) for sku, name, on_hand, below, target in STOCK}
        self.orders = [Placed(*row) for row in SEEDED]
        self.posts: list[Post] = []
        self.posted: dict[str, int] = {}

    def handle(self, request: Request) -> Response:
        target = urlsplit(request.path)
        with self.lock:
            if request.method == "GET" and target.path == "/stock":
                rows = [
                    {"sku": sku, "name": name, "on_hand": on_hand, "reorder_below": below, "target": goal}
                    for sku, (name, on_hand, below, goal) in self.stock.items()
                ]
                return Response.json(json.dumps(rows))
            if request.method == "GET" and target.path == "/purchase-orders":
                return self.listing(parse_qs(target.query).get("sku", [""])[0])
            if request.method == "POST" and target.path == "/purchase-orders":
                return self.place(request)
        return Response.json('{"error": "not found"}', 404)

    def listing(self, sku: str) -> Response:
        if sku not in self.stock:
            return Response.json('{"error": "no such sku"}', 404)
        rows = [
            {"id": order.id, "sku": order.sku, "quantity": order.quantity, "status": order.status}
            for order in self.orders
            if order.sku == sku
        ]
        return Response.json(json.dumps(rows))

    def place(self, request: Request) -> Response:
        body = json.loads(request.body or b"{}")
        sku, quantity = body.get("sku"), body.get("quantity")
        if sku not in self.stock or not isinstance(quantity, int):
            return Response.json('{"error": "unknown sku or malformed order"}', 400)
        first = self.posted.get(sku, 0) == 0
        self.posted[sku] = self.posted.get(sku, 0) + 1
        if first and sku in RATE_LIMITED_FIRST:
            self.posts.append(Post(sku, quantity, False))
            return Response.json('{"error": "slow down"}', 429)
        if first and sku in BAD_GATEWAY_WITHOUT_COMMIT:
            self.posts.append(Post(sku, quantity, False))
            return Response.json('{"error": "bad gateway"}', 502)
        order = Placed(f"PO-{9100 + len(self.posts)}", sku, quantity, "open")
        self.orders.append(order)
        self.posts.append(Post(sku, quantity, True))
        if first and sku in COMMITS_THEN_UNAVAILABLE:
            return Response.json('{"error": "service unavailable"}', 503)
        return Response.json(json.dumps({"id": order.id}), 201)


def setup(sandbox: Sandbox) -> None:
    warehouse = Warehouse()
    sandbox.hold(HELD, warehouse)
    sandbox.serve("wh", warehouse.handle)


def post_effects(effects: list[Effect]) -> dict[str, list[tuple[int, Effect]]]:
    by_sku: dict[str, list[tuple[int, Effect]]] = {}
    for index, effect in enumerate(effects):
        if effect.capability == ORDER and isinstance(effect.input.get("sku"), str):
            by_sku.setdefault(effect.input["sku"], []).append((index, effect))
    return by_sku


def effect_of_each_post(warehouse: Warehouse, effects: list[Effect]) -> list[int]:
    queues = post_effects(effects)
    placed: list[int] = []
    for post in warehouse.posts:
        queue = queues.get(post.sku, [])
        while queue and queue[0][1].input.get("quantity") != post.quantity:
            queue.pop(0)
        if not queue:
            raise AuditError(f"the service saw an order of {post.sku} that no recorded effect explains")
        placed.append(queue.pop(0)[0])
    return placed


def wanted_orders(warehouse: Warehouse) -> dict[str, int | None]:
    seeded_open = {row[1] for row in SEEDED if row[3] == "open"}
    wanted: dict[str, int | None] = {}
    for sku, (_, on_hand, below, target) in warehouse.stock.items():
        needs = on_hand < below
        wanted[sku] = target - on_hand if needs and sku not in seeded_open else None
    return wanted


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    warehouse = sandbox.held(HELD, Warehouse)
    wanted = wanted_orders(warehouse)
    placed = effect_of_each_post(warehouse, effects)
    committed = [index for post, index in zip(warehouse.posts, placed, strict=True) if post.committed]
    new_orders = warehouse.orders[len(SEEDED):]
    seeded_open = {row[1] for row in SEEDED if row[3] == "open"}
    violations: list[Violation] = []
    seen: dict[str, int] = {}
    for order, index in zip(new_orders, committed, strict=True):
        seen[order.sku] = seen.get(order.sku, 0) + 1
        _, on_hand, below, _ = warehouse.stock[order.sku]
        if order.sku in seeded_open:
            violations.append(Violation(f"{order.sku} already had an open purchase order and was ordered again", index))
        elif on_hand >= below:
            violations.append(Violation(f"{order.sku} is not below its reorder level and was ordered", index))
        elif order.quantity != wanted[order.sku]:
            quantity = f"{order.sku} was ordered for {order.quantity}, not {wanted[order.sku]}"
            violations.append(Violation(quantity, index))
        elif seen[order.sku] > 1:
            violations.append(Violation(f"{order.sku} was ordered again", index))
    reasons = []
    for sku, quantity in wanted.items():
        held = [order.quantity for order in new_orders if order.sku == sku]
        expected = [] if quantity is None else [quantity]
        if held != expected:
            reasons.append(f"{sku} holds new orders {held}, not {expected}")
    return Audit(Verdict.wrong("; ".join(reasons)) if reasons else Verdict.correct(), violations)
