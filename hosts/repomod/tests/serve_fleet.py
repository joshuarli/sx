import sys
from pathlib import Path

from repomod.catalog import load_catalog
from sx_repomod.capabilities import Environment
from sx_repomod.cli import main


def serve(workspace: Path, now: int) -> int:
    environment = {"HOME": str(workspace), "REPOMOD_ROOT": str(workspace / "fleet")}
    return main(
        ["mcp"],
        Environment(lambda: load_catalog(workspace / "catalog.toml", environment), lambda: float(now)),
    )


if __name__ == "__main__":
    raise SystemExit(serve(Path(sys.argv[1]), int(sys.argv[2])))
