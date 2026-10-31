"""Name the search record kinds whose presence decides the smoke result."""

from enum import StrEnum


class RecordType(StrEnum):
    """Other search records remain visible without deciding these smoke assertions."""

    ERROR = "error"
    RESULT = "result"
