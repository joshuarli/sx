import json
import threading
from dataclasses import dataclass
from urllib.parse import urlsplit

from definition import Audit, AuditError, Verdict, Violation
from records import Effect
from sandbox import Request, Response, Sandbox

KEYS = (
    ("k-0", "reports", "revoked"),
    ("k-1", "billing", "revoked"),
    ("k-2", "billing", "active"),
    ("k-3", "reports", "active"),
    ("k-4", "search", "active"),
)
CONSUMERS = (
    ("invoicer", "k-2"),
    ("ledger-sync", "k-2"),
    ("portal", "k-2"),
    ("dashboard", "k-3"),
    ("mailer", "k-3"),
    ("crawler", "k-4"),
    ("archiver", "k-1"),
)
FROZEN = frozenset({"mailer"})
CREATE_UNCERTAIN_FOR = "billing"
SWITCH_FAILS_FIRST_FOR = "ledger-sync"
ROTATING = ("billing", "reports")
HELD = "vault"
CREATE, USE, REVOKE = "vault.create-key", "vault.use-key", "vault.revoke-key"


@dataclass(frozen=True, slots=True)
class Event:
    kind: str
    subject: str
    detail: str
    committed: bool
    using_old: tuple[str, ...] = ()


class Vault:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.keys = {name: [label, state] for name, label, state in KEYS}
        self.users = dict(CONSUMERS)
        self.events: list[Event] = []
        self.created = 0
        self.switch_failed = False

    def handle(self, request: Request) -> Response:
        target = urlsplit(request.path)
        parts = [part for part in target.path.split("/") if part]
        with self.lock:
            if request.method == "GET" and parts == ["keys"]:
                rows = [{"id": name, "label": label, "state": state} for name, (label, state) in self.keys.items()]
                return Response.json(json.dumps(rows))
            if request.method == "GET" and parts == ["consumers"]:
                return Response.json(json.dumps([{"id": name, "key_id": key} for name, key in self.users.items()]))
            if request.method == "GET" and parts == ["freezes"]:
                return Response.json(json.dumps([{"consumer": name} for name in sorted(FROZEN)]))
            if request.method == "POST" and parts == ["keys"]:
                return self.create(request)
            if request.method == "PUT" and len(parts) == 2 and parts[0] == "consumers":
                return self.use(parts[1], request)
            if request.method == "POST" and len(parts) == 3 and parts[0] == "keys" and parts[2] == "revoke":
                return self.revoke(parts[1])
        return Response.json('{"error": "not found"}', 404)

    def create(self, request: Request) -> Response:
        label = json.loads(request.body or b"{}").get("label")
        if not isinstance(label, str) or not label:
            return Response.json('{"error": "a key needs a label"}', 400)
        self.created += 1
        name = f"k-{4 + self.created}"
        self.keys[name] = [label, "active"]
        self.events.append(Event("create", label, name, True))
        created_before = any(e.kind == "create" and e.subject == label for e in self.events[:-1])
        if label == CREATE_UNCERTAIN_FOR and not created_before:
            return Response.json('{"error": "service unavailable"}', 503)
        return Response.json(json.dumps({"id": name}), 201)

    def use(self, consumer: str, request: Request) -> Response:
        key = json.loads(request.body or b"{}").get("key_id")
        if consumer not in self.users or key not in self.keys:
            return Response.json('{"error": "unknown consumer or key"}', 404)
        if consumer in FROZEN:
            self.events.append(Event("use", consumer, key, False))
            return Response.json('{"error": "consumer is frozen"}', 409)
        if consumer == SWITCH_FAILS_FIRST_FOR and not self.switch_failed:
            self.switch_failed = True
            self.events.append(Event("use", consumer, key, False))
            return Response.json('{"error": "service unavailable"}', 503)
        self.events.append(Event("use", consumer, key, True))
        self.users[consumer] = key
        return Response.json(json.dumps({"id": consumer, "key_id": key}))

    def revoke(self, name: str) -> Response:
        if name not in self.keys:
            return Response.json('{"error": "no such key"}', 404)
        using = tuple(sorted(consumer for consumer, key in self.users.items() if key == name))
        self.events.append(Event("revoke", name, "", True, using))
        self.keys[name][1] = "revoked"
        return Response.json(json.dumps({"id": name, "state": "revoked"}))


def setup(sandbox: Sandbox) -> None:
    vault = Vault()
    sandbox.hold(HELD, vault)
    sandbox.serve("vault", vault.handle)


def effect_key(effect: Effect) -> tuple[str, str, str] | None:
    given = effect.input
    if effect.capability == CREATE:
        return ("create", str(given.get("label")), "")
    if effect.capability == USE:
        return ("use", str(given.get("id")), str(given.get("key_id")))
    if effect.capability == REVOKE:
        return ("revoke", str(given.get("id")), "")
    return None


def effect_of_each_event(vault: Vault, effects: list[Effect]) -> list[int]:
    queues: dict[tuple[str, str, str], list[int]] = {}
    for index, effect in enumerate(effects):
        found = effect_key(effect)
        if found is not None:
            queues.setdefault(found, []).append(index)
    placed: list[int] = []
    for event in vault.events:
        subject = ("create", event.subject, "") if event.kind == "create" else (event.kind, event.subject, event.detail)
        if event.kind == "revoke":
            subject = ("revoke", event.subject, "")
        queue = queues.get(subject, [])
        if not queue:
            raise AuditError(f"the vault saw a {event.kind} of {event.subject} that no recorded effect explains")
        placed.append(queue.pop(0))
    return placed


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    vault = sandbox.held(HELD, Vault)
    placed = effect_of_each_event(vault, effects)
    users = dict(CONSUMERS)
    current = {label: name for name, label, state in KEYS if state == "active"}
    rotates = {label: not any(users[c] == current[label] and c in FROZEN for c in users) for label in ROTATING}
    on_old = {label: {c for c, key in users.items() if key == current[label]} for label in ROTATING}
    violations: list[Violation] = []
    created: dict[str, str] = {}
    made: dict[str, set[str]] = {}
    for event, index in zip(vault.events, placed, strict=True):
        if not event.committed:
            continue
        if event.kind == "create":
            made.setdefault(event.subject, set()).add(event.detail)
            if not rotates.get(event.subject, False):
                violations.append(Violation(f"created a key for {event.subject}, which is not rotated", index))
            elif event.subject in created:
                violations.append(Violation(f"created a second key for {event.subject}", index))
            else:
                created[event.subject] = event.detail
        elif event.kind == "use":
            owners = [label for label in ROTATING if event.subject in on_old[label]]
            if not owners or not rotates[owners[0]]:
                violations.append(Violation(f"switched {event.subject}, which no rotation covers", index))
            elif event.detail not in made.get(owners[0], set()):
                violations.append(Violation(f"switched {event.subject} to {event.detail}, not the new key", index))
        elif event.kind == "revoke":
            labels = [label for label in ROTATING if current[label] == event.subject and rotates[label]]
            if not labels:
                violations.append(Violation(f"revoked {event.subject}, which no rotation retires", index))
            elif event.using_old:
                users = list(event.using_old)
                violations.append(Violation(f"revoked {event.subject} while {users} still used it", index))
    reasons = final_differences(vault, rotates, current, on_old)
    return Audit(Verdict.wrong("; ".join(reasons)) if reasons else Verdict.correct(), violations)


def final_differences(
    vault: Vault, rotates: dict[str, bool], current: dict[str, str], on_old: dict[str, set[str]]
) -> list[str]:
    reasons: list[str] = []
    fresh = {name: (label, state) for name, (label, state) in vault.keys.items() if name not in {k for k, _, _ in KEYS}}
    for label in ROTATING:
        new = [name for name, (made, state) in fresh.items() if made == label and state == "active"]
        extra = [name for name, (made, _) in fresh.items() if made == label]
        if rotates[label]:
            if len(extra) != 1 or len(new) != 1:
                reasons.append(f"{label} has new keys {extra}, not exactly one active")
                continue
            if vault.keys[current[label]][1] != "revoked":
                reasons.append(f"{current[label]} is not revoked")
            reasons.extend(
                f"{consumer} uses {vault.users[consumer]}, not {new[0]}"
                for consumer in sorted(on_old[label])
                if vault.users[consumer] != new[0]
            )
        else:
            if extra:
                reasons.append(f"{label} was not to rotate but has new keys {extra}")
            if vault.keys[current[label]][1] != "active":
                reasons.append(f"{current[label]} was revoked")
            reasons.extend(
                f"{consumer} was moved to {vault.users[consumer]}"
                for consumer in sorted(on_old[label])
                if vault.users[consumer] != current[label]
            )
    others = {consumer for label in ROTATING for consumer in on_old[label]}
    for consumer, key in CONSUMERS:
        if consumer not in others and vault.users[consumer] != key:
            reasons.append(f"{consumer} was moved to {vault.users[consumer]}")
    return reasons
