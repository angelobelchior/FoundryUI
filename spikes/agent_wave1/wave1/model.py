import http.client
import ipaddress
import json
from dataclasses import dataclass
from urllib.parse import urlsplit

from wave1.protocol import ToolProtocolError, ToolStream


@dataclass(frozen=True)
class ModelProbeResult:
    status: str
    model: str
    tool_calls: int = 0
    reason: str | None = None
    product_ready: bool = False
    thinking_disabled: bool = False


def probe_local_model(endpoint: str, model: str, disable_thinking: bool = False) -> ModelProbeResult:
    try:
        parts = urlsplit(endpoint)
        if parts.scheme != "http" or not parts.hostname or parts.username is not None or parts.password is not None:
            raise ValueError("unsupported_endpoint")
        if not ipaddress.ip_address(parts.hostname).is_loopback or parts.query or parts.fragment or parts.path.rstrip("/") not in {"", "/v1"}:
            raise ValueError("unsupported_endpoint")
        if any(ord(char) < 33 or ord(char) > 126 for char in endpoint):
            raise ValueError("unsupported_endpoint")
        port = parts.port
        if port is not None and not 1 <= port <= 65535:
            raise ValueError("invalid_port")
    except ValueError:
        return ModelProbeResult("blocked", model, reason="literal_loopback_endpoint_required", thinking_disabled=disable_thinking)

    definitions = {"lookup_fixture": {"key": str}}
    tool = {"type": "function", "function": {"name": "lookup_fixture", "description": "Read a value from a fixed in-memory fixture; no files or commands.", "parameters": {"type": "object", "properties": {"key": {"type": "string"}}, "required": ["key"], "additionalProperties": False}}}
    prompt = "Call lookup_fixture with key sample exactly once, then return its result exactly. Do not invent the result."
    if disable_thinking:
        prompt += " /no_think"
    messages = [{"role": "user", "content": prompt}]

    def request():
        connection = http.client.HTTPConnection(parts.hostname, port, timeout=15)
        try:
            payload = {"model": model, "messages": messages, "tools": [tool], "stream": True, "max_tokens": 256, "temperature": 0}
            connection.request("POST", "/v1/chat/completions", json.dumps(payload).encode(), {"Content-Type": "application/json", "Connection": "close"})
            response = connection.getresponse()
            if response.status != 200 or "text/event-stream" not in response.getheader("Content-Type", ""):
                raise ToolProtocolError("unsupported_response")
            stream = ToolStream(definitions)
            while chunk := response.read(4096):
                stream.feed(chunk)
            return stream.finish()
        finally:
            connection.close()

    try:
        first = request()
        if len(first.calls) != 1 or first.calls[0].arguments != {"key": "sample"}:
            return ModelProbeResult("not_proven", model, reason="expected_fixture_call_not_observed", thinking_disabled=disable_thinking)
        call = first.calls[0]
        messages.extend([
            {"role": "assistant", "content": first.content or None, "tool_calls": [{"id": call.id, "type": "function", "function": {"name": call.name, "arguments": json.dumps(call.arguments)}}]},
            {"role": "tool", "tool_call_id": call.id, "content": "fixture-value"},
        ])
        second = request()
        if second.calls or second.content.strip() != "fixture-value":
            return ModelProbeResult("not_proven", model, 1, "continuation_not_observed", thinking_disabled=disable_thinking)
        return ModelProbeResult("cycle_observed", model, 1, thinking_disabled=disable_thinking)
    except ToolProtocolError as error:
        return ModelProbeResult("not_proven", model, reason=str(error), thinking_disabled=disable_thinking)
    except (OSError, ValueError, http.client.HTTPException):
        return ModelProbeResult("not_proven", model, reason="transport_or_protocol_failure", thinking_disabled=disable_thinking)
