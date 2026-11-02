import json
import random
from collections import defaultdict
from decimal import ROUND_HALF_UP, Decimal

from definition import Evidence, Verdict
from sandbox import Sandbox

CUSTOMERS = ("Acme Corp", "Birch & Sons", "Cobalt Ltd", "Delta Foods", "Evergreen Co")
PRICES = ("19.99", "5.5", "0.125", "249", "3.333", "0.05", "12.75", "7.045", "100.1", "1.005", "2.675", "0.285", "8.345")
DISCOUNTS = ("0", "0", "10", "12.5", "5", "33.3", "0")
INVOICES = 22
WRONG_TOTAL = {3: "0.01", 7: "-1.00", 12: "0.10", 17: "100.00"}
TWO_PLACES = Decimal("0.01")
type Invoice = tuple[str, str, str, list[tuple[str, str, str, str]]]


def line_amount(quantity: str, price: str, discount: str) -> Decimal:
    exact = Decimal(quantity) * Decimal(price) * (Decimal(100) - Decimal(discount)) / Decimal(100)
    return exact.quantize(TWO_PLACES, rounding=ROUND_HALF_UP)


def invoices() -> dict[str, Invoice]:
    generator = random.Random(1717)
    found: dict[str, Invoice] = {}
    for number in range(INVOICES):
        identifier = f"INV-{2001 + number}"
        customer = CUSTOMERS[(number * 3 + number // 5) % len(CUSTOMERS)]
        lines = [
            (
                f"SKU-{generator.randrange(100, 999)}",
                str(generator.randrange(1, 40)),
                generator.choice(PRICES),
                generator.choice(DISCOUNTS),
            )
            for _ in range(generator.randrange(2, 7))
        ]
        if number % 6 == 0:
            lines.append(("SKU-TIE", "1", "0.125", "0"))
        total = sum((line_amount(q, p, d) for _, q, p, d in lines), Decimal(0))
        stated = total + Decimal(WRONG_TOTAL.get(number, "0"))
        shown = f"{stated:.2f}" if number % 4 else f"{stated.normalize():f}"
        found[identifier] = (identifier, customer, shown, lines)
    return found


def render(invoice: Invoice, blank: bool) -> str:
    identifier, customer, stated, lines = invoice
    rows = [f"invoice,{identifier},{customer},{stated}", *(",".join(line) for line in lines)]
    return "\n".join(rows) + ("\n\n" if blank else "\n")


def computed(invoice: Invoice) -> Decimal:
    return sum((line_amount(q, p, d) for _, q, p, d in invoice[3]), Decimal(0))


def expected() -> object:
    totals: dict[str, Decimal] = defaultdict(Decimal)
    counts: dict[str, int] = defaultdict(int)
    mismatched = []
    for identifier, invoice in sorted(invoices().items()):
        total = computed(invoice)
        totals[invoice[1]] += total
        counts[invoice[1]] += 1
        if total != Decimal(invoice[2]):
            mismatched.append(
                {"invoice": identifier, "stated": f"{Decimal(invoice[2]):.2f}", "computed": f"{total:.2f}"}
            )
    customers = [
        {"customer": name, "invoices": counts[name], "total": f"{totals[name]:.2f}"} for name in sorted(totals)
    ]
    return {"customers": customers, "mismatched": mismatched}


def setup(sandbox: Sandbox) -> None:
    for number, (identifier, invoice) in enumerate(invoices().items()):
        sandbox.write(f"invoices/{identifier}.csv", render(invoice, blank=number % 5 == 0))
    sandbox.write("invoices/README.txt", "invoice,INV-9999,Ghost Ltd,1.00\nSKU-1,1,1,0\n")
    sandbox.write("invoices/INV-2001.csv.bak", "invoice,INV-9998,Ghost Ltd,1.00\nSKU-1,1,1,0\n")


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    wanted = expected()
    if result != wanted:
        return Verdict.wrong(f"the result was {json.dumps(result)}, not {json.dumps(wanted)}")
    names = sorted(path.name for path in sandbox.root.iterdir())
    if names != ["invoices"]:
        return Verdict.wrong(f"the program left {names} in the sandbox")
    return Verdict.correct()
