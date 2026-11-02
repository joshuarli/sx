from __future__ import annotations

import re


def camel(name: str) -> str:
    return re.sub(r"-([a-z0-9])", lambda found: found.group(1).upper(), name)
