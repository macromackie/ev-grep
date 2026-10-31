"""Own loopback listener acquisition, finite connections, and joined serving threads."""

import threading
from contextlib import contextmanager
from functools import partial
from socketserver import TCPServer
from typing import TYPE_CHECKING, override

from release.constants import (
    CONNECTION_TIMEOUT_SECONDS,
    HOST,
    SERVER_JOIN_TIMEOUT_SECONDS,
    SERVER_POLL_SECONDS,
)
from release.errors import SmokeFailed
from release.provider import Provider

if TYPE_CHECKING:
    import socket
    import ssl
    from collections.abc import Generator


class ProviderServer(TCPServer):
    """Accept finite loopback connections without HTTPServer's reverse DNS lookup."""

    def __init__(self, *, authorizations: list[str], context: ssl.SSLContext | None) -> None:
        """Bind one ephemeral listener; an absent context selects the plain HTTP case."""
        self._context = context

        super().__init__((HOST, 0), partial(Provider, authorizations=authorizations))

    @override
    # TCPServer calls this method by its fixed name.
    # ast-grep-ignore: lookup-names-python,function-verbs-python
    def get_request(self) -> tuple[socket.socket, tuple[str, int]]:
        connection, address = self.socket.accept()

        connection.settimeout(CONNECTION_TIMEOUT_SECONDS)

        if self._context is None:
            return connection, address

        return self._context.wrap_socket(connection, server_side=True), address

    def _stop(self, *, thread: threading.Thread) -> None:
        self.shutdown()
        thread.join(timeout=SERVER_JOIN_TIMEOUT_SECONDS)

        if thread.is_alive():
            message = "stop provider thread 'release-smoke-provider'"
            raise SmokeFailed(message)

    @contextmanager
    def serve_requests(self) -> Generator[None]:
        """Join this listener's serving thread before its socket owner closes it."""
        thread = threading.Thread(
            name="release-smoke-provider",
            target=partial(self.serve_forever, poll_interval=SERVER_POLL_SECONDS),
        )

        thread.start()

        try:
            yield
        finally:
            self._stop(thread=thread)


@contextmanager
def serve(*, context: ssl.SSLContext | None) -> Generator[tuple[int, list[str]]]:
    """Close the listener and join its thread even when a smoke case raises."""
    authorizations: list[str] = []

    with (
        ProviderServer(authorizations=authorizations, context=context) as server,
        server.serve_requests(),
    ):
        yield server.server_address[1], authorizations
