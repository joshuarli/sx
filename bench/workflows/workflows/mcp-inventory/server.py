import json
import sys

CATALOG: dict[str, dict[str, object]] = {
    "A-100": {"name": "Fresh basil", "cents": 250, "tags": ["perishable", "herb"]},
    "B-200": {"name": "Copper kettle", "cents": 4200, "tags": ["kitchen"]},
    "C-300": {"name": "Oak shelf", "cents": 1850, "tags": ["furniture"]},
    "E-500": {"name": "Oat milk", "cents": 199, "tags": ["perishable"]},
}
STOCK = {"A-100": 5, "B-200": 10, "C-300": 4, "E-500": 2}
PROTOCOLS = ("2025-06-18", "2025-03-26", "2024-11-05")
SKU = {"type": "string"}
TOOLS = [
    {
        "name": "lookup_item",
        "description": "Returns the catalog entry of one item.",
        "inputSchema": {"type": "object", "properties": {"sku": SKU}, "required": ["sku"]},
        "outputSchema": {
            "type": "object",
            "properties": {
                "sku": SKU,
                "name": {"type": "string"},
                "cents": {"type": "integer"},
                "stock": {"type": "integer"},
                "tags": {"type": "array", "items": {"type": "string"}},
            },
            "required": ["sku", "name", "cents", "stock", "tags"],
        },
        "annotations": {"readOnlyHint": True},
    },
    {
        "name": "reserve_stock",
        "description": "Reserves units of one item and returns the reservation.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "sku": SKU,
                "quantity": {"type": "integer"},
                "priority": {"type": "string", "enum": ["normal", "rush"]},
            },
            "required": ["sku", "quantity"],
        },
        "outputSchema": {
            "type": "object",
            "properties": {"reservation": {"type": "string"}, "remaining": {"type": "integer"}},
            "required": ["reservation", "remaining"],
        },
        "annotations": {"readOnlyHint": False, "destructiveHint": True, "idempotentHint": False},
    },
]


class Inventory:
    def __init__(self, ledger: str) -> None:
        self.ledger = ledger
        self.stock = dict(STOCK)
        self.reservations = 0

    def record(self, name: str, arguments: dict[str, object]) -> None:
        with open(self.ledger, "a") as handle:
            handle.write(json.dumps({"tool": name, "arguments": arguments}) + "\n")

    def call(self, name: str, arguments: dict[str, object]) -> dict[str, object]:
        self.record(name, arguments)
        sku = str(arguments.get("sku"))
        if sku not in CATALOG:
            return failure(f"unknown sku: {sku}")
        if name == "lookup_item":
            entry: dict[str, object] = {"sku": sku, **CATALOG[sku], "stock": self.stock[sku]}
            return success(entry)
        quantity = arguments.get("quantity")
        if not isinstance(quantity, int) or quantity < 1 or quantity > self.stock[sku]:
            return failure(f"cannot reserve {quantity} of {sku}: {self.stock[sku]} in stock")
        self.stock[sku] -= quantity
        self.reservations += 1
        return success({"reservation": f"R-{self.reservations}", "remaining": self.stock[sku]})


def success(value: dict[str, object]) -> dict[str, object]:
    return {"content": [{"type": "text", "text": json.dumps(value)}], "structuredContent": value}


def failure(message: str) -> dict[str, object]:
    return {"content": [{"type": "text", "text": message}], "isError": True}


def respond(identifier: object, result: object = None, error: tuple[int, str] | None = None) -> None:
    message: dict[str, object] = {"jsonrpc": "2.0", "id": identifier}
    if error is not None:
        message["error"] = {"code": error[0], "message": error[1]}
    else:
        message["result"] = result
    sys.stdout.write(json.dumps(message) + "\n")
    sys.stdout.flush()


def serve(inventory: Inventory) -> None:
    for line in sys.stdin:
        request = json.loads(line)
        method, identifier = request.get("method"), request.get("id")
        if identifier is None:
            continue
        if method == "initialize":
            asked = request.get("params", {}).get("protocolVersion")
            version = asked if asked in PROTOCOLS else PROTOCOLS[0]
            respond(identifier, {
                "protocolVersion": version,
                "capabilities": {"tools": {"listChanged": False}},
                "serverInfo": {"name": "inventory", "version": "1"},
            })
        elif method == "ping":
            respond(identifier, {})
        elif method == "tools/list":
            respond(identifier, {"tools": TOOLS})
        elif method == "tools/call":
            params = request.get("params", {})
            name = params.get("name")
            if name not in {tool["name"] for tool in TOOLS}:
                respond(identifier, error=(-32602, f"unknown tool: {name}"))
            else:
                respond(identifier, inventory.call(name, params.get("arguments", {})))
        else:
            respond(identifier, error=(-32601, f"method not found: {method}"))


if __name__ == "__main__":
    serve(Inventory(sys.argv[1]))
