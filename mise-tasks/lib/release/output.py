"""Own the smoke program's report, failure diagnostics, and process termination."""

import sys
from typing import TYPE_CHECKING, NoReturn

if TYPE_CHECKING:
    from release.exit_status import ExitStatus


def finish(*, report: tuple[str, ...], status: ExitStatus, error: str | None = None) -> NoReturn:
    """Emit completed evidence on stdout and a prefixed refusal on stderr before exiting."""
    for line in report:
        print(line, flush=True)

    if error is not None:
        for line in error.splitlines():
            print(f"release:smoke: {line}", file=sys.stderr, flush=True)

    sys.exit(status)
