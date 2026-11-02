import json
import sqlite3

from definition import Evidence, Verdict
from sandbox import Sandbox

CUSTOMERS = [
    (1, "bob@example.com", "Bob", 5),
    (2, "Carol@Example.COM", "Carol", 7),
    (3, "ana@example.com", "Ana B", 20),
    (4, "dan@example.com", "Dan", 9),
    (5, " carol@example.com ", "Carol Smith", 3),
    (6, "erin@example.com", "Erin", 11),
    (7, "ERIN@example.com", "Erin R", 11),
    (8, "Ana@example.com", "Ana", 10),
    (9, "frank@example.com", "Frank", 2),
    (10, "bob@EXAMPLE.com", "Robert", 4),
    (11, "gina@example.com", "Gina", 8),
    (12, "ana@Example.com", "Ana C", 10),
    (13, "Henry@Example.com", "Henry", 6),
]
ORDERS = [(n, 1 + (n * 5) % 13, 1000 + n * 175 % 4000) for n in range(1, 27)]
NOTES = [
    (1, 2, "prefers email"),
    (2, 5, "prefers email"),
    (3, 5, "net 30"),
    (4, 7, "vip"),
    (5, 6, "call back"),
    (6, 3, "call back"),
    (7, 12, "call back"),
    (8, 3, "net 30"),
    (9, 12, "net 30"),
    (10, 1, "wholesale"),
    (11, 10, "wholesale"),
    (12, 8, "call back"),
    (13, 4, "late payer"),
]
SCHEMA = """
CREATE TABLE customers (
  id INTEGER PRIMARY KEY, email TEXT NOT NULL, name TEXT NOT NULL, created INTEGER NOT NULL);
CREATE TABLE orders (
  id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), cents INTEGER NOT NULL);
CREATE TABLE notes (
  id INTEGER PRIMARY KEY, customer_id INTEGER NOT NULL REFERENCES customers(id), body TEXT NOT NULL,
  UNIQUE (customer_id, body));
CREATE TABLE audit (
  id INTEGER PRIMARY KEY AUTOINCREMENT, tbl TEXT NOT NULL, action TEXT NOT NULL, row_id INTEGER NOT NULL);
CREATE TRIGGER customers_update AFTER UPDATE ON customers BEGIN
  INSERT INTO audit(tbl, action, row_id) VALUES ('customers', 'update', OLD.id);
END;
CREATE TRIGGER customers_delete AFTER DELETE ON customers BEGIN
  INSERT INTO audit(tbl, action, row_id) VALUES ('customers', 'delete', OLD.id);
END;
CREATE TRIGGER orders_update AFTER UPDATE ON orders BEGIN
  INSERT INTO audit(tbl, action, row_id) VALUES ('orders', 'update', OLD.id);
END;
CREATE TRIGGER notes_update AFTER UPDATE ON notes BEGIN
  INSERT INTO audit(tbl, action, row_id) VALUES ('notes', 'update', OLD.id);
END;
CREATE TRIGGER notes_delete AFTER DELETE ON notes BEGIN
  INSERT INTO audit(tbl, action, row_id) VALUES ('notes', 'delete', OLD.id);
END;
"""
type Row = dict[str, object]


def normalize(email: str) -> str:
    return email.strip().lower()


def expected() -> tuple[Row, dict[str, list[tuple[int | str, ...]]], list[tuple[str, str, int]]]:
    emails = {i: e for i, e, _, _ in CUSTOMERS}
    created = {i: c for i, _, _, c in CUSTOMERS}
    owner = {i: c for i, c, _ in ORDERS}
    holder = {i: c for i, c, _ in NOTES}
    bodies = {i: b for i, _, b in NOTES}
    audit: list[tuple[str, str, int]] = []
    groups: dict[str, list[int]] = {}
    for i, e, _, _ in CUSTOMERS:
        groups.setdefault(normalize(e), []).append(i)
    plans = []
    for members in groups.values():
        if len(members) > 1:
            ordered = sorted(members, key=lambda i: (created[i], i))
            plans.append((ordered[0], sorted(ordered[1:])))
    plans.sort()
    merged = []
    for survivor, removed in plans:
        moved_orders = moved_notes = dropped_notes = 0
        for source in removed:
            for order_id in [o for o, c in owner.items() if c == source]:
                owner[order_id] = survivor
                moved_orders += 1
                audit.append(("orders", "update", order_id))
            for note_id in sorted(n for n, c in holder.items() if c == source):
                if any(holder[n] == survivor and bodies[n] == bodies[note_id] for n in holder):
                    del holder[note_id]
                    dropped_notes += 1
                    audit.append(("notes", "delete", note_id))
                else:
                    holder[note_id] = survivor
                    moved_notes += 1
                    audit.append(("notes", "update", note_id))
            del emails[source]
            audit.append(("customers", "delete", source))
        merged.append(
            {
                "survivor": survivor,
                "removed": removed,
                "orders": moved_orders,
                "notes_moved": moved_notes,
                "notes_dropped": dropped_notes,
            }
        )
    changed = sorted(i for i, e in emails.items() if e != normalize(e))
    for i in changed:
        emails[i] = normalize(emails[i])
        audit.append(("customers", "update", i))
    result = {"merged": merged, "emails_changed": changed, "customers": len(emails)}
    names = {i: n for i, _, n, _ in CUSTOMERS}
    cents = {i: c for i, _, c in ORDERS}
    tables: dict[str, list[tuple[int | str, ...]]] = {
        "customers": sorted((i, e, names[i], created[i]) for i, e in emails.items()),
        "orders": sorted((i, c, cents[i]) for i, c in owner.items()),
        "notes": sorted((i, c, bodies[i]) for i, c in holder.items()),
    }
    return result, tables, sorted(audit)


def setup(sandbox: Sandbox) -> None:
    connection = sqlite3.connect(sandbox.root / "crm.db")
    try:
        connection.executescript(SCHEMA)
        connection.executemany("INSERT INTO customers VALUES (?, ?, ?, ?)", CUSTOMERS)
        connection.executemany("INSERT INTO orders VALUES (?, ?, ?)", ORDERS)
        connection.executemany("INSERT INTO notes VALUES (?, ?, ?)", NOTES)
        connection.execute("DELETE FROM audit")
        connection.commit()
    finally:
        connection.close()


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted, tables, audit = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    connection = sqlite3.connect(sandbox.root / "crm.db")
    try:
        found = {
            "customers": sorted(connection.execute("SELECT id, email, name, created FROM customers").fetchall()),
            "orders": sorted(connection.execute("SELECT id, customer_id, cents FROM orders").fetchall()),
            "notes": sorted(connection.execute("SELECT id, customer_id, body FROM notes").fetchall()),
        }
        trail = sorted(connection.execute("SELECT tbl, action, row_id FROM audit").fetchall())
    finally:
        connection.close()
    for name, rows in tables.items():
        if found[name] != rows:
            return Verdict.wrong(f"the {name} table holds {found[name]}, not {rows}")
    if trail != audit:
        return Verdict.wrong(f"the changes made were {trail}, not {audit}")
    extra = sorted(item.name for item in sandbox.root.iterdir() if item.name != "crm.db")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
