import json
import threading
from dataclasses import dataclass
from urllib.parse import parse_qs, urlsplit

from definition import Audit, AuditError, Verdict, Violation
from records import Effect
from sandbox import Request, Response, Sandbox

CUSTOMERS = (
    ("c-ava", "north", False),
    ("c-bo", "south", False),
    ("c-cy", "north", True),
    ("c-di", "east", False),
    ("c-eli", "south", False),
    ("c-flo", "east", False),
)
ORDERS = (
    ("o-1", "c-ava", 1200, "ready"),
    ("o-2", "c-bo", 800, "ready"),
    ("o-3", "c-ava", 450, "ready"),
    ("o-4", "c-cy", 900, "ready"),
    ("o-5", "c-di", 2300, "ready"),
    ("o-6", "c-ava", 300, "hold"),
    ("o-7", "c-ava", 700, "ready"),
    ("o-8", "c-bo", 1500, "shipped"),
    ("o-9", "c-eli", 640, "ready"),
    ("o-10", "c-ava", 980, "ready"),
    ("o-11", "c-di", 410, "ready"),
    ("o-12", "c-ava", 125, "ready"),
    ("o-13", "c-flo", 3000, "hold"),
    ("o-14", "c-bo", 220, "ready"),
    ("o-15", "c-eli", 1750, "ready"),
    ("o-16", "c-ava", 2600, "ready"),
)
MAX_ORDERS = 4
COMMITS_THEN_UNAVAILABLE = frozenset({"o-1", "o-5"})
UNAVAILABLE_WITHOUT_COMMIT = frozenset({"o-12"})
RATE_LIMITED = frozenset({"o-9"})
BOOK = "ship.book"
HELD = "depot"


@dataclass(frozen=True, slots=True)
class Booking:
    id: str
    customer: str
    zone: str
    orders: tuple[str, ...]
    weight_g: int


@dataclass(frozen=True, slots=True)
class Post:
    booking: Booking
    committed: bool


class Depot:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.zones = {name: zone for name, zone, _ in CUSTOMERS}
        self.blocked = {name for name, _, blocked in CUSTOMERS if blocked}
        self.orders = {name: (customer, grams, status) for name, customer, grams, status in ORDERS}
        self.bookings: list[Booking] = []
        self.posts: list[Post] = []
        self.tried: set[str] = set()

    def handle(self, request: Request) -> Response:
        target = urlsplit(request.path)
        with self.lock:
            if request.method == "GET" and target.path == "/orders":
                rows = [
                    {"id": name, "customer": customer, "weight_g": grams, "status": status}
                    for name, (customer, grams, status) in self.orders.items()
                ]
                return Response.json(json.dumps(rows))
            if request.method == "GET" and target.path == "/customers":
                rows = [{"id": name, "zone": zone, "blocked": blocked} for name, zone, blocked in CUSTOMERS]
                return Response.json(json.dumps(rows))
            if request.method == "GET" and target.path == "/bookings":
                customer = parse_qs(target.query).get("customer", [""])[0]
                return Response.json(json.dumps([row(b) for b in self.bookings if b.customer == customer]))
            if request.method == "POST" and target.path == "/bookings":
                return self.book(request)
        return Response.json('{"error": "not found"}', 404)

    def book(self, request: Request) -> Response:
        body = json.loads(request.body or b"{}")
        customer, zone, orders, grams = body.get("customer"), body.get("zone"), body.get("orders"), body.get("weight_g")
        if not (
            customer in self.zones and isinstance(zone, str) and isinstance(grams, int)
            and isinstance(orders, list) and orders and all(isinstance(name, str) for name in orders)
        ):
            return Response.json('{"error": "malformed booking"}', 400)
        booking = Booking(f"bk-{len(self.bookings) + 1}", customer, zone, tuple(orders), grams)
        first = booking.orders[0] not in self.tried
        self.tried.add(booking.orders[0])
        if first and booking.orders[0] in RATE_LIMITED:
            return Response.json('{"error": "slow down"}', 429)
        if first and booking.orders[0] in UNAVAILABLE_WITHOUT_COMMIT:
            self.posts.append(Post(booking, False))
            return Response.json('{"error": "service unavailable"}', 503)
        self.bookings.append(booking)
        self.posts.append(Post(booking, True))
        if first and booking.orders[0] in COMMITS_THEN_UNAVAILABLE:
            return Response.json('{"error": "service unavailable"}', 503)
        return Response.json(json.dumps({"id": booking.id}), 201)


def row(booking: Booking) -> dict[str, object]:
    return {
        "id": booking.id, "customer": booking.customer, "orders": list(booking.orders), "weight_g": booking.weight_g,
    }


def setup(sandbox: Sandbox) -> None:
    depot = Depot()
    sandbox.hold(HELD, depot)
    sandbox.serve("ship", depot.handle)


def expected_bookings(depot: Depot) -> set[tuple[str, tuple[str, ...]]]:
    ready: dict[str, list[str]] = {}
    for name, (customer, _, status) in depot.orders.items():
        if status == "ready" and customer not in depot.blocked:
            ready.setdefault(customer, []).append(name)
    return {
        (customer, tuple(names[start:start + MAX_ORDERS]))
        for customer, names in ready.items()
        for start in range(0, len(names), MAX_ORDERS)
    }


def effect_of_each_post(depot: Depot, effects: list[Effect]) -> list[int]:
    queues: dict[tuple[str, tuple[str, ...]], list[int]] = {}
    for index, effect in enumerate(effects):
        orders = effect.input.get("orders")
        if effect.capability == BOOK and isinstance(orders, list) and isinstance(effect.input.get("customer"), str):
            queues.setdefault((effect.input["customer"], tuple(orders)), []).append(index)
    placed: list[int] = []
    for post in depot.posts:
        queue = queues.get((post.booking.customer, post.booking.orders), [])
        if not queue:
            raise AuditError(f"the depot saw a booking of {post.booking.orders} that no recorded effect explains")
        placed.append(queue.pop(0))
    return placed


def problems(depot: Depot, booking: Booking, booked: set[str]) -> list[str]:
    found: list[str] = []
    if booking.customer in depot.blocked:
        found.append(f"{booking.customer} is blocked")
    again = sorted(booked & set(booking.orders))
    if again:
        found.append(f"{again} were booked before")
    for name in booking.orders:
        owner, _, status = depot.orders.get(name, ("", 0, ""))
        if status != "ready":
            found.append(f"{name} is {status or 'unknown'}")
        elif owner != booking.customer:
            found.append(f"{name} belongs to {owner}")
    if len(booking.orders) > MAX_ORDERS:
        found.append(f"{len(booking.orders)} orders in one booking")
    if booking.zone != depot.zones[booking.customer]:
        found.append(f"zone {booking.zone}, not {depot.zones[booking.customer]}")
    grams = sum(depot.orders[name][1] for name in booking.orders if name in depot.orders)
    if booking.weight_g != grams:
        found.append(f"weight {booking.weight_g}, not {grams}")
    return found


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    depot = sandbox.held(HELD, Depot)
    placed = effect_of_each_post(depot, effects)
    committed = [index for post, index in zip(depot.posts, placed, strict=True) if post.committed]
    booked: set[str] = set()
    violations: list[Violation] = []
    for booking, index in zip(depot.bookings, committed, strict=True):
        for reason in problems(depot, booking, booked):
            violations.append(Violation(f"booking {booking.id} of {booking.customer}: {reason}", index))
        booked |= set(booking.orders)
    held = sorted((b.customer, b.orders) for b in depot.bookings)
    wanted = sorted(expected_bookings(depot))
    if held == wanted:
        return Audit(Verdict.correct(), violations)
    missing = [pair for pair in wanted if pair not in held]
    extra = [pair for pair in held if pair not in wanted]
    repeated = sorted({pair for pair in held if held.count(pair) > 1})
    return Audit(Verdict.wrong(f"missing {missing}, unexpected {extra}, repeated {repeated}"), violations)
