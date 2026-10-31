"""Exercise verified TLS, rejected TLS, and plain HTTP through the same packaged binary."""

import json
import shutil
import ssl
import subprocess
import tempfile
from pathlib import Path

from release.constants import (
    CERTIFICATE_COMMANDS,
    ERROR_KIND_TLS,
    HOST,
    KEY,
    OPENSSL_TIMEOUT_SECONDS,
    QUERY,
    REQUEST_CONFIG,
    SEARCH_TIMEOUT_SECONDS,
    SERVER_EXTENSIONS,
)
from release.errors import SmokeFailed
from release.exit_status import ExitStatus
from release.record_type import RecordType
from release.server import serve
from release.types import SmokeCase


def _prepare_certificates(*, directory: Path, executable: str) -> ssl.SSLContext:
    (directory / "request.cnf").write_text(REQUEST_CONFIG, encoding="utf-8")
    (directory / "server.cnf").write_text(SERVER_EXTENSIONS, encoding="utf-8")
    (directory / "sample.rs").write_text("fn main() {}\n", encoding="utf-8")

    for command in CERTIFICATE_COMMANDS:
        completed = subprocess.run(
            [executable, *command.split()],
            capture_output=True,
            check=False,
            cwd=directory,
            text=True,
            timeout=OPENSSL_TIMEOUT_SECONDS,
        )

        if completed.returncode != ExitStatus.PASSED:
            message = f"generate smoke certificate {command!r}: {completed.stderr}"
            raise SmokeFailed(message)

    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)

    context.load_cert_chain(directory / "server.pem", directory / "server.key")

    return context


def _prepare(*, directory: Path) -> ssl.SSLContext:
    executable = shutil.which("openssl")

    if executable is None:
        message = "locate smoke certificate generator 'openssl'"
        raise SmokeFailed(message)

    return _prepare_certificates(directory=directory, executable=executable)


def _read_record(*, line: str) -> tuple[str, str | None]:
    record: object = json.loads(line)

    match record:
        case {"data": {"kind": str(kind)}, "type": RecordType.ERROR}:
            return RecordType.ERROR.value, kind
        case {"type": str(record_type)}:
            return record_type, None
        case _:
            message = f"decode search record {line!r}"
            raise SmokeFailed(message)


def _search(
    *, authority: Path | None, binary: Path, directory: Path, endpoint: str
) -> subprocess.CompletedProcess[str]:
    environment = {"HOME": str(directory), "OPENROUTER_API_KEY": KEY}

    if authority is not None:
        environment["SSL_CERT_FILE"] = str(authority)

    return subprocess.run(
        [str(binary), "--json", "--endpoint", endpoint, QUERY, "sample.rs"],
        capture_output=True,
        check=False,
        cwd=directory,
        env=environment,
        text=True,
        timeout=SEARCH_TIMEOUT_SECONDS,
    )


def _validate_records(
    *, completed: subprocess.CompletedProcess[str], expected_record: RecordType
) -> tuple[tuple[str, ...], list[str]]:
    records = [_read_record(line=line) for line in completed.stdout.splitlines()]
    record_types = tuple(record_type for record_type, _ in records)
    failures: list[str] = []

    if record_types.count(expected_record) != 1:
        failures.append(f"expected one {expected_record!r} record")

    if expected_record == RecordType.ERROR:
        error_kinds = [kind for record_type, kind in records if record_type == RecordType.ERROR]

        if error_kinds != [ERROR_KIND_TLS]:
            failures.append("expected a tls error")

    return record_types, failures


def _validate(
    *,
    authorizations: list[str],
    completed: subprocess.CompletedProcess[str],
    expected_record: RecordType,
    expected_requests: int,
    expected_status: ExitStatus,
) -> tuple[str, ...]:
    record_types, failures = _validate_records(completed=completed, expected_record=expected_record)

    if completed.returncode != expected_status:
        failures.append(f"expected exit {expected_status!r}")

    if authorizations != [f"Bearer {KEY}"] * expected_requests:
        failures.append(f"expected {expected_requests!r} requests with the fake key")

    if failures:
        message = (
            f"verify search {completed.args!r}: {'; '.join(failures)}"
            f"\nstdout:\n{completed.stdout}\nstderr:\n{completed.stderr}"
        )
        raise SmokeFailed(message)

    return record_types


def _run_case(*, binary: Path, case: SmokeCase, directory: Path) -> str:
    sent_requests = len(case.authorizations)
    completed = _search(
        authority=case.authority, binary=binary, directory=directory, endpoint=case.endpoint
    )
    received_authorizations = case.authorizations[sent_requests:]
    record_types = _validate(
        authorizations=received_authorizations,
        completed=completed,
        expected_record=case.expected_record,
        expected_requests=case.expected_requests,
        expected_status=case.expected_status,
    )

    return (
        f"{case.label}: exit {completed.returncode}, records {list(record_types)}, "
        f"{len(received_authorizations)} requests"
    )


def _run_cases(*, binary: Path, context: ssl.SSLContext, directory: Path) -> tuple[str, ...]:
    with (
        serve(context=context) as (secure_port, secure_authorizations),
        serve(context=None) as (
            plain_port,
            plain_authorizations,
        ),
    ):
        cases = (
            SmokeCase(
                authority=directory / "ca.pem",
                authorizations=secure_authorizations,
                endpoint=f"https://{HOST}:{secure_port}/",
                expected_record=RecordType.RESULT,
                expected_requests=1,
                expected_status=ExitStatus.PASSED,
                label="HTTPS with SSL_CERT_FILE",
            ),
            SmokeCase(
                authority=None,
                authorizations=secure_authorizations,
                endpoint=f"https://{HOST}:{secure_port}/",
                expected_record=RecordType.ERROR,
                expected_requests=0,
                expected_status=ExitStatus.SEARCH_ERROR,
                label="HTTPS without the certificate authority",
            ),
            SmokeCase(
                authority=None,
                authorizations=plain_authorizations,
                endpoint=f"http://{HOST}:{plain_port}/",
                expected_record=RecordType.RESULT,
                expected_requests=1,
                expected_status=ExitStatus.PASSED,
                label="plain HTTP",
            ),
        )

        return tuple(_run_case(binary=binary, case=case, directory=directory) for case in cases)


def run(*, binary: Path) -> tuple[str, ...]:
    """Require the three original protocol outcomes and exact authenticated request counts."""
    with tempfile.TemporaryDirectory(prefix="ev-grep-release-smoke-") as name:
        directory = Path(name)
        context = _prepare(directory=directory)

        return _run_cases(binary=binary, context=context, directory=directory)
