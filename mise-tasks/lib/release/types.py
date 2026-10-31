"""Keep each protocol case's expected evidence with its selected local provider."""

from dataclasses import dataclass
from typing import TYPE_CHECKING

if TYPE_CHECKING:
    from pathlib import Path

    from release.exit_status import ExitStatus
    from release.record_type import RecordType


@dataclass(frozen=True, kw_only=True)
class SmokeCase:
    """An absent authority deliberately tests rejection or plain HTTP without custom trust."""

    authority: Path | None
    authorizations: list[str]
    endpoint: str
    expected_record: RecordType
    expected_requests: int
    expected_status: ExitStatus
    label: str
