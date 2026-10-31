"""Answer decision requests without exposing the smoke key beyond loopback."""

import json
from http import HTTPStatus
from http.server import BaseHTTPRequestHandler
from typing import TYPE_CHECKING, override

from release.errors import SmokeFailed

if TYPE_CHECKING:
    import socket
    from socketserver import BaseServer


def _read_request(*, body: bytes) -> tuple[str, tuple[str, ...]]:
    request: object = json.loads(body)
    message = f"decode provider request {body!r}"

    match request:
        case {"model": str(model), "questions": dict(questions)}:
            question_names: list[str] = []

            for question in questions:
                if not isinstance(question, str):
                    raise SmokeFailed(message)

                question_names.append(question)

            return model, tuple(question_names)
        case _:
            raise SmokeFailed(message)


class Provider(BaseHTTPRequestHandler):
    """Record authorization only after a request reaches HTTP, including over verified TLS."""

    @override
    def __init__(
        self,
        request: socket.socket | tuple[bytes, socket.socket],
        client_address: object,
        server: BaseServer,
        *,
        authorizations: list[str] | None = None,
    ) -> None:
        """Require the recorder while preserving the HTTP server's constructor signature."""
        if authorizations is None:
            message = "compose provider 'authorizations'"
            raise SmokeFailed(message)

        self._authorizations = authorizations

        super().__init__(request, client_address, server)

    def _read_body(self) -> bytes:
        length = self.headers["Content-Length"]

        if length is None:
            message = "read provider request 'Content-Length'"
            raise SmokeFailed(message)

        return self.rfile.read(int(length))

    def _write_response(self, *, body: bytes) -> None:
        self.send_response(HTTPStatus.OK)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    # BaseHTTPRequestHandler dispatches HTTP POST by this exact method name.
    # ast-grep-ignore: acronym-names-python,function-verbs-python
    def do_POST(self) -> None:
        """Return one confident match for every question using the request's own model."""
        model, questions = _read_request(body=self._read_body())
        authorization = self.headers["Authorization"]

        if authorization is None:
            message = "read provider request 'Authorization'"
            raise SmokeFailed(message)

        self._authorizations.append(authorization)

        answer = {
            "choice": "match",
            "confidence": 0.9,
            "probabilities": {"match": 0.9, "no_match": 0.05, "uncertain": 0.05},
            "type": "choice",
        }
        body = json.dumps({
            "answers": dict.fromkeys(questions, answer),
            "model": model,
            "usage": {"input_tokens": 20, "output_tokens": 4},
        }).encode()

        self._write_response(body=body)

    @override
    def log_message(self, format: str, *args: object) -> None:
        return
