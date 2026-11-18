import http.client
import ipaddress
from collections.abc import Callable
from dataclasses import dataclass
from datetime import datetime, timezone
from urllib.parse import urljoin, urlsplit, urlunsplit


@dataclass(frozen=True)
class AuditContext:
    conversation_id: str
    run_id: str
    call_id: str


@dataclass(frozen=True)
class NetworkEvent:
    context: AuditContext
    destination: str
    state: str
    timestamp: str
    http_status: int | None = None
    reason: str | None = None


@dataclass(frozen=True)
class NetworkResult:
    status: str
    http_status: int | None
    body: bytes
    events: tuple[NetworkEvent, ...]


def sanitized_destination(url: str) -> str:
    try:
        parts = urlsplit(url)
        host = parts.hostname
        if not host or any(ord(char) < 33 or ord(char) > 126 for char in host):
            return "[invalid-destination]"
        authority = f"[{host}]" if ":" in host else host
        if parts.port:
            authority += f":{parts.port}"
        return urlunsplit((parts.scheme, authority, "/[redacted]" if parts.path not in {"", "/"} else parts.path, "[redacted]" if parts.query else "", ""))
    except (ValueError, UnicodeError):
        return "[invalid-destination]"


class MediatedClient:
    def __init__(self, timeout: float = 2, max_body: int = 65_536, max_redirects: int = 4):
        self._timeout = timeout
        self._max_body = max_body
        self._max_redirects = max_redirects

    def fetch(self, url: str, authorize: Callable[[str], bool], context: AuditContext) -> NetworkResult:
        events: list[NetworkEvent] = []

        def record(state: str, code: int | None = None, reason: str | None = None) -> None:
            events.append(NetworkEvent(context, sanitized_destination(url), state, datetime.now(timezone.utc).isoformat(), code, reason))

        for hop in range(self._max_redirects + 1):
            record("requested")
            try:
                if len(url) > 8192 or any(ord(char) < 33 or ord(char) > 126 for char in url):
                    raise ValueError("invalid_url")
                parts = urlsplit(url)
                if parts.scheme not in {"http", "https"} or not parts.hostname or parts.username is not None or parts.password is not None:
                    raise ValueError("unsupported_url")
                if parts.port is not None and not 1 <= parts.port <= 65535:
                    raise ValueError("invalid_port")
                if parts.scheme == "http" and not ipaddress.ip_address(parts.hostname).is_loopback:
                    raise ValueError("plaintext_requires_literal_loopback")
            except (ValueError, UnicodeError):
                record("blocked", reason="unsupported_or_invalid_url")
                return NetworkResult("blocked", None, b"", tuple(events))
            try:
                allowed = authorize(url) is True
            except Exception:
                allowed = False
            if not allowed:
                record("blocked", reason="denied")
                return NetworkResult("blocked", None, b"", tuple(events))
            connection = None
            try:
                connection = (http.client.HTTPSConnection if parts.scheme == "https" else http.client.HTTPConnection)(parts.hostname, parts.port, timeout=self._timeout)
                record("attempted")
                connection.request("GET", urlunsplit(("", "", parts.path or "/", parts.query, "")), headers={"Accept": "application/octet-stream", "Connection": "close"})
                response = connection.getresponse()
                if response.status in {301, 302, 303, 307, 308}:
                    location = response.getheader("Location")
                    if not location or hop == self._max_redirects:
                        record("failed", response.status, "redirect_limit_or_missing_location")
                        return NetworkResult("failed", response.status, b"", tuple(events))
                    record("redirected", response.status)
                    url = urljoin(url, location)
                    continue
                body = response.read(self._max_body + 1)
                if len(body) > self._max_body:
                    record("failed", response.status, "body_limit")
                    return NetworkResult("failed", response.status, b"", tuple(events))
                record("completed", response.status)
                return NetworkResult("completed", response.status, body, tuple(events))
            except (OSError, ValueError, http.client.HTTPException):
                record("failed", reason="transport_error")
                return NetworkResult("failed", None, b"", tuple(events))
            finally:
                if connection:
                    connection.close()
        raise RuntimeError("unreachable_redirect_state")
