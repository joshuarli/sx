from collections.abc import Generator, Mapping, Sequence
from contextlib import contextmanager
from typing import Any

from sx_repomod.kernel import Kernel


class Evaluator:
    def __init__(self, kernel: Kernel, cores: Sequence[str]) -> None:
        self.kernel = kernel
        self.cores = list(cores)

    def __call__(self, source: str, inputs: Mapping[str, tuple[str, object]] | None = None) -> Any:
        declared = inputs or {}
        reply = self.kernel.send(
            {
                "type": "start",
                "program": source,
                "declarations": {"cores": self.cores, "inputs": {name: kind for name, (kind, _) in declared.items()}},
                "allow": [],
                "inputs": {name: value for name, (_, value) in declared.items()},
            }
        )
        assert reply["type"] == "completed", reply
        return reply["value"]


@contextmanager
def evaluator(*cores: str) -> Generator[Evaluator]:
    with Kernel() as kernel:
        yield Evaluator(kernel, cores)
