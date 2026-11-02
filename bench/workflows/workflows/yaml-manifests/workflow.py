from definition import Evidence, Verdict
from sandbox import Sandbox

ORIGINAL = {
    "api.yaml": (
        "# Public API service\n"
        "metadata:\n"
        "  name: api   # do not rename\n"
        "  version: '2.0.0'\n"
        "  owner: platform\n"
        "\n"
        "spec:\n"
        "  replicas: 1\n"
        "  image:\n"
        "    repo: registry.local/api\n"
        "    tag: 2.0.0 # follows the version\n"
    ),
    "cron.yaml": (
        "metadata:\n"
        "  name: cron\n"
        "  version: 3.1.4\n"
        "  owner: frozen\n"
        "spec:\n"
        "  replicas: 1\n"
        "  image:\n"
        "    repo: registry.local/cron\n"
        "    tag: 3.1.4\n"
    ),
    "db.yaml": (
        "---\n"
        "metadata:\n"
        "  owner: data\n"
        "  name: db\n"
        "  version: \"7.12.99\"\n"
        "spec:\n"
        "  replicas: 2\n"
        "  volumes:\n"
        "    - name: data\n"
        "      size: 10Gi\n"
        "    - name: logs\n"
        "      size: 1Gi\n"
    ),
    "legacy.yaml": (
        "metadata: {name: legacy, version: 0.1.0, owner: platform}\n"
        "spec:\n"
        "  replicas: 1\n"
        "  ports: [80, 443]\n"
    ),
    "web.yaml": (
        "metadata:\n"
        "  name: web\n"
        "  version: \"1.4.9\"\n"
        "  owner: platform\n"
        "spec:\n"
        "  replicas: 3\n"
        "  image:\n"
        "    repo: registry.local/web\n"
        "    tag: \"1.4.8\"\n"
        "  env:\n"
        "    - name: MODE\n"
        "      value: prod\n"
    ),
    "worker.yaml": (
        "metadata:\r\n"
        "  name: worker\r\n"
        "  version: 0.9.12\r\n"
        "  owner: jobs\r\n"
        "spec:\r\n"
        "  replicas: 1\r\n"
        "  image:\r\n"
        "    repo: registry.local/worker\r\n"
        "    tag: 0.9.12\r\n"
    ),
}
EXPECTED = {
    "api.yaml": (
        ORIGINAL["api.yaml"].replace("'2.0.0'", "'2.0.1'").replace("replicas: 1", "replicas: 2")
        .replace("tag: 2.0.0 #", "tag: 2.0.1 #")
    ),
    "cron.yaml": ORIGINAL["cron.yaml"],
    "db.yaml": ORIGINAL["db.yaml"].replace('"7.12.99"', '"7.12.100"'),
    "legacy.yaml": ORIGINAL["legacy.yaml"],
    "web.yaml": ORIGINAL["web.yaml"].replace('"1.4.9"', '"1.4.10"'),
    "worker.yaml": (
        ORIGINAL["worker.yaml"].replace("version: 0.9.12", "version: 0.9.13").replace("replicas: 1", "replicas: 2")
        .replace("tag: 0.9.12", "tag: 0.9.13")
    ),
}
RESULT = {
    "updated": [
        {"file": "api.yaml", "version": "2.0.1"},
        {"file": "db.yaml", "version": "7.12.100"},
        {"file": "web.yaml", "version": "1.4.10"},
        {"file": "worker.yaml", "version": "0.9.13"},
    ],
    "skipped": ["cron.yaml"],
}


def setup(sandbox: Sandbox) -> None:
    for name, text in ORIGINAL.items():
        path = sandbox.write(f"manifests/{name}", "")
        path.write_bytes(text.encode())


def check(sandbox: Sandbox, result: object, evidence: Evidence) -> Verdict:
    if result != RESULT:
        return Verdict.wrong(f"the result was {result!r}, not {RESULT!r}")
    directory = sandbox.root / "manifests"
    names = sorted(path.name for path in directory.iterdir())
    if names != sorted(ORIGINAL):
        return Verdict.wrong(f"the manifests directory holds {names}, not {sorted(ORIGINAL)}")
    for name, text in EXPECTED.items():
        found = (directory / name).read_bytes()
        if found != text.encode():
            return Verdict.wrong(f"{name} is {found!r}, not {text.encode()!r}")
    extra = sorted(path.name for path in sandbox.root.iterdir() if path.name != "manifests")
    if extra:
        return Verdict.wrong(f"the program left {extra} in the sandbox")
    return Verdict.correct()
