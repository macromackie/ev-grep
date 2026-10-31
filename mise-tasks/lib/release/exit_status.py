"""Declare the smoke program's process outcomes once."""

from enum import IntEnum


class ExitStatus(IntEnum):
    """A smoke run either rejects its evidence or completes all three protocol cases."""

    FAILED = 1
    PASSED = 0
    SEARCH_ERROR = 2
