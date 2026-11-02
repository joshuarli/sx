import json
import re
import threading
from dataclasses import dataclass
from urllib.parse import parse_qs, urlsplit

from definition import Audit, AuditError, Verdict, Violation
from records import Effect
from sandbox import Request, Response, Sandbox

ALERTS = (
    ("al-01", "billing", "info", "2026-03-14T06:02:00Z", "nightly invoice run finished"),
    ("al-02", "search", "warning", "2026-03-14T07:15:00Z", "index lag above 90 seconds"),
    ("al-03", "auth", "critical", "2026-03-14T07:48:00Z", "login error rate above 20 percent"),
    ("al-04", "storage", "info", "2026-03-14T08:05:00Z", "snapshot completed"),
    ("al-05", "gateway", "warning", "2026-03-14T08:31:00Z", "p99 latency above 2 seconds"),
    ("al-06", "billing", "critical", "2026-03-14T09:12:00Z", "payment provider timeouts"),
    ("al-07", "queue", "warning", "2026-03-14T09:40:00Z", "backlog above 10000 jobs"),
    ("al-08", "search", "info", "2026-03-14T10:03:00Z", "reindex scheduled"),
    ("al-09", "storage", "critical", "2026-03-14T10:26:00Z", "primary volume at 97 percent"),
    ("al-10", "auth", "warning", "2026-03-14T10:58:00Z", "token refresh retries rising"),
    ("al-11", "gateway", "info", "2026-03-14T11:20:00Z", "certificate renewed"),
    ("al-12", "queue", "critical", "2026-03-14T11:47:00Z", "dead letter queue growing"),
    ("al-13", "billing", "warning", "2026-03-14T12:09:00Z", "refund backlog above 50"),
    ("al-14", "search", "info", "2026-03-14T12:34:00Z", "cache warmed"),
    ("al-15", "storage", "warning", "2026-03-14T13:01:00Z", "replica lag above 30 seconds"),
    ("al-16", "gateway", "critical", "2026-03-14T13:22:00Z", "5xx rate above 5 percent"),
    ("al-17", "auth", "info", "2026-03-14T13:50:00Z", "key rotation due next week"),
    ("al-18", "queue", "warning", "2026-03-14T14:11:00Z", "consumer restarts detected"),
    ("al-19", "billing", "info", "2026-03-14T14:36:00Z", "tax tables updated"),
    ("al-20", "search", "warning", "2026-03-14T14:58:00Z", "query errors above 1 percent"),
)
LISTED_ORDER = (11, 2, 15, 6, 19, 0, 8, 13, 4, 17, 1, 10, 7, 18, 3, 12, 9, 16, 5, 14)
RATE_LIMITED_ONCE = frozenset({"al-12"})
RECORDED_THEN_UNAVAILABLE = frozenset({"al-09"})
CHANNEL = "ops"
POST = "chat.post"
HELD = "chat"
TOP = 3
ALERT_ID = re.compile(r"\bal-\d+\b")


@dataclass(frozen=True, slots=True)
class Message:
    id: str
    channel: str
    text: str


@dataclass(frozen=True, slots=True)
class Post:
    channel: str
    text: str
    committed: bool


class Chat:
    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.alerts = [ALERTS[position] for position in LISTED_ORDER]
        self.messages: list[Message] = []
        self.posts: list[Post] = []
        self.seen: set[str] = set()

    def handle(self, request: Request) -> Response:
        target = urlsplit(request.path)
        with self.lock:
            if request.method == "GET" and target.path == "/alerts":
                return self.listing()
            if request.method == "GET" and target.path == "/messages":
                return self.listed(parse_qs(target.query).get("channel", [""])[0])
            if request.method == "POST" and target.path == "/messages":
                return self.post(request)
        return Response.json('{"error": "not found"}', 404)

    def listing(self) -> Response:
        rows = [
            {"id": name, "service": service, "severity": severity, "raised_at": raised, "text": text}
            for name, service, severity, raised, text in self.alerts
        ]
        return Response.json(json.dumps(rows))

    def listed(self, channel: str) -> Response:
        rows = [message_row(message) for message in self.messages if message.channel == channel]
        return Response.json(json.dumps(rows))

    def post(self, request: Request) -> Response:
        body = json.loads(request.body or b"{}")
        channel, text = body.get("channel"), body.get("text")
        if not isinstance(channel, str) or not isinstance(text, str):
            return Response.json('{"error": "malformed message"}', 400)
        mentioned = set(ALERT_ID.findall(text))
        first = not (mentioned & self.seen)
        self.seen |= mentioned
        if first and mentioned & RATE_LIMITED_ONCE:
            self.posts.append(Post(channel, text, False))
            return Response.json('{"error": "slow down"}', 429)
        message = Message(f"msg-{len(self.messages) + 1}", channel, text)
        self.messages.append(message)
        self.posts.append(Post(channel, text, True))
        if first and mentioned & RECORDED_THEN_UNAVAILABLE:
            return Response.json('{"error": "service unavailable"}', 503)
        return Response.json(json.dumps({"id": message.id}), 201)


def message_row(message: Message) -> dict[str, str]:
    return {"id": message.id, "channel": message.channel, "text": message.text}


def setup(sandbox: Sandbox) -> None:
    chat = Chat()
    sandbox.hold(HELD, chat)
    sandbox.serve("alerts", chat.handle)
    sandbox.serve("chat", chat.handle)


def newest_critical(chat: Chat) -> list[str]:
    critical = [alert for alert in chat.alerts if alert[2] == "critical"]
    return [alert[0] for alert in sorted(critical, key=lambda alert: alert[3], reverse=True)[:TOP]]


def effect_of_each_post(chat: Chat, effects: list[Effect]) -> list[int]:
    queue = [(index, effect) for index, effect in enumerate(effects) if effect.capability == POST]
    placed: list[int] = []
    for post in chat.posts:
        position = next(
            (
                at
                for at, (_, effect) in enumerate(queue)
                if effect.input.get("channel") == post.channel and effect.input.get("text") == post.text
            ),
            None,
        )
        if position is None:
            raise AuditError(f"the service saw a post to {post.channel} that no recorded effect explains")
        placed.append(queue.pop(position)[0])
    return placed


def audit(sandbox: Sandbox, effects: list[Effect]) -> Audit:
    chat = sandbox.held(HELD, Chat)
    wanted = newest_critical(chat)
    placed = effect_of_each_post(chat, effects)
    committed = [index for post, index in zip(chat.posts, placed, strict=True) if post.committed]
    announced: set[str] = set()
    violations: list[Violation] = []
    for number, (message, index) in enumerate(zip(chat.messages, committed, strict=True), start=1):
        mentioned = ALERT_ID.findall(message.text)
        alert = mentioned[0] if len(mentioned) == 1 else None
        if message.channel != CHANNEL:
            violations.append(Violation(f"a message was posted to {message.channel}, not {CHANNEL}", index))
        elif alert in announced:
            violations.append(Violation(f"{alert} was announced again", index))
        elif alert not in wanted:
            named = alert or "no single alert"
            violations.append(Violation(f"a message announces {named}, not one of {wanted}", index))
        elif number > TOP:
            violations.append(Violation(f"message {number} is beyond the {TOP} that are allowed", index))
        if alert is not None:
            announced.add(alert)
    posted = [message.text for message in chat.messages if message.channel == CHANNEL]
    reasons = []
    if len(chat.messages) != len(posted):
        reasons.append("a message went to another channel")
    held = [ALERT_ID.findall(text) for text in posted]
    if sorted(found[0] if len(found) == 1 else "" for found in held) != sorted(wanted):
        reasons.append(f"{CHANNEL} announces {held}, not exactly {wanted}")
    return Audit(Verdict.wrong("; ".join(reasons)) if reasons else Verdict.correct(), violations)
