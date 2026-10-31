"""Distinguish rejected smoke evidence from an unexpected Python failure."""


class SmokeFailed(Exception):
    """The packaged binary or its local provider failed a required smoke invariant."""
