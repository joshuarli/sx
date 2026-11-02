PYTHON_MARKER = "; python: no"


def source_runs_in_python(source: str) -> bool:
    for line in source.split("\n"):
        if not line.startswith(";"):
            return True
        if line == PYTHON_MARKER:
            return False
    return True
