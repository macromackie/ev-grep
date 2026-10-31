"""Keep the fake protocol and its finite process and connection budgets together."""

BINARY_ARGUMENTS = 2
CONNECTION_TIMEOUT_SECONDS = 5
ERROR_KIND_TLS = "tls"
HOST = "127.0.0.1"
KEY = "release-smoke-key"
OPENSSL_TIMEOUT_SECONDS = 30
QUERY = "contains a main function"
REQUEST_CONFIG = """\
[req]
distinguished_name = name
[name]
[authority]
basicConstraints = critical, CA:TRUE
keyUsage = critical, keyCertSign
"""
SEARCH_TIMEOUT_SECONDS = 60
SERVER_EXTENSIONS = """\
basicConstraints = critical, CA:FALSE
keyUsage = critical, digitalSignature, keyEncipherment
extendedKeyUsage = serverAuth
subjectAltName = IP:127.0.0.1
"""
SERVER_JOIN_TIMEOUT_SECONDS = 2
SERVER_POLL_SECONDS = 0.1
CERTIFICATE_COMMANDS = (
    (
        "req -x509 -newkey rsa:2048 -nodes -sha256 -days 1 -subj /CN=ev-grep-release-smoke-ca"
        " -config request.cnf -extensions authority -keyout ca.key -out ca.pem"
    ),
    (
        "req -new -newkey rsa:2048 -nodes -subj /CN=127.0.0.1 -config request.cnf"
        " -keyout server.key -out server.csr"
    ),
    (
        "x509 -req -sha256 -days 1 -set_serial 2 -in server.csr -CA ca.pem -CAkey ca.key"
        " -extfile server.cnf -out server.pem"
    ),
)
