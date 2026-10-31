"""Read the standalone program's binary argument before creating local resources."""

import sys
from pathlib import Path

from release.constants import BINARY_ARGUMENTS
from release.errors import SmokeFailed


def read_binary() -> Path:
    """Require one binary path and resolve it before searches change their working directory."""
    if len(sys.argv) != BINARY_ARGUMENTS:
        message = "read binary argument; run 'mise run release:smoke <binary>'"
        raise SmokeFailed(message)

    return Path(sys.argv[1]).resolve(strict=True)
