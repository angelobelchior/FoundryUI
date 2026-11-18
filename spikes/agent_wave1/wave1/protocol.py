import json
import math
from dataclasses import dataclass
from typing import Any


class ToolProtocolError(ValueError):
    pass


def unique_object(pairs: list[tuple[str, Any]]) -> dict[str, Any]:
    result: dict[str, Any] = {}
    for key, value in pairs:
        if key in result:
            raise ToolProtocolError("duplicate_json_key")
        result[key] = value
    return result


def reject_constant(value: str) -> Any:
    raise ToolProtocolError("nonstandard_json_number")


def finite_float(value: str) -> float:
    number = float(value)
    if not math.isfinite(number):
        raise ToolProtocolError("nonfinite_json_number")
    return number


@dataclass(frozen=True)
class ToolCall:
    id: str
    name: str
    arguments: dict[str, Any]


@dataclass(frozen=True)
class StreamResult:
    calls: tuple[ToolCall, ...]
    content: str
    reasoning: str


class ToolStream:
    def __init__(self, definitions: dict[str, dict[str, type]]):
        self._definitions = definitions
        self._wire = bytearray()
        self._failed = False

    def feed(self, chunk: bytes) -> None:
        if self._failed or len(self._wire) + len(chunk) > 1_048_576:
            self._failed = True
            raise ToolProtocolError("stream_limit")
        self._wire.extend(chunk)

    def finish(self) -> StreamResult:
        if self._failed:
            raise ToolProtocolError("invalidated_stream")
        pending: dict[int, dict[str, str]] = {}
        content: list[str] = []
        reasoning: list[str] = []
        completed = False
        finish_reason = None
        try:
            wire = self._wire.decode("utf-8").replace("\r\n", "\n").replace("\r", "\n")
            if not wire.endswith("\n\n"):
                raise ToolProtocolError("unfinished_sse_event")
            for block in wire.split("\n\n"):
                if not block.strip():
                    continue
                data = "\n".join(line[5:].lstrip(" ") for line in block.splitlines() if line.startswith("data:"))
                if not data:
                    continue
                if completed:
                    raise ToolProtocolError("data_after_done")
                if data == "[DONE]":
                    completed = True
                    continue
                payload = json.loads(data, object_pairs_hook=unique_object, parse_constant=reject_constant, parse_float=finite_float)
                if not isinstance(payload, dict) or "error" in payload:
                    raise ToolProtocolError("provider_error_or_invalid_envelope")
                choices = payload.get("choices")
                if not isinstance(choices, list) or (not choices and not isinstance(payload.get("usage"), dict)):
                    raise ToolProtocolError("invalid_choices_envelope")
                for choice in choices:
                    if finish_reason:
                        raise ToolProtocolError("data_after_finish")
                    if choice.get("index", 0) != 0:
                        raise ToolProtocolError("multiple_choices_unsupported")
                    delta = choice.get("delta", {})
                    content.append(delta.get("content") or "")
                    reasoning.append(delta.get("reasoning_content") or "")
                    for part in delta.get("tool_calls", []):
                        if part.get("type") not in {None, "function"}:
                            raise ToolProtocolError("unsupported_tool_type")
                        if type(part["index"]) is not int or not 0 <= part["index"] < 16:
                            raise ToolProtocolError("invalid_call_index")
                        if part.get("id") is not None and (not isinstance(part["id"], str) or not part["id"]):
                            raise ToolProtocolError("invalid_call_id")
                        entry = pending.setdefault(part["index"], {"id": "", "name": "", "arguments": ""})
                        if part.get("id"):
                            if entry["id"] and entry["id"] != part["id"]:
                                raise ToolProtocolError("changed_call_id")
                            entry["id"] = part["id"]
                        function = part.get("function", {})
                        for field in ("name", "arguments"):
                            fragment = function.get(field)
                            if fragment is not None:
                                if not isinstance(fragment, str):
                                    raise ToolProtocolError("invalid_function_fragment")
                                entry[field] += fragment
                        if len(entry["arguments"]) > 65_536 or len(entry["name"]) > 128 or len(entry["id"]) > 256:
                            raise ToolProtocolError("call_limit")
                    if choice.get("finish_reason"):
                        finish_reason = choice["finish_reason"]
            if not completed or finish_reason not in {"stop", "tool_calls"}:
                raise ToolProtocolError("incomplete_stream")
            if pending and finish_reason != "tool_calls":
                raise ToolProtocolError("incomplete_calls")
            if finish_reason == "tool_calls" and not pending:
                raise ToolProtocolError("missing_calls")
            calls = []
            for index in sorted(pending):
                entry = pending[index]
                name = entry["name"]
                if not entry["id"] or name not in self._definitions:
                    raise ToolProtocolError("unknown_or_incomplete_tool")
                if any(call.id == entry["id"] for call in calls):
                    raise ToolProtocolError("duplicate_call_id")
                arguments = json.loads(entry["arguments"], object_pairs_hook=unique_object, parse_constant=reject_constant, parse_float=finite_float)
                definition = self._definitions[name]
                if not isinstance(arguments, dict) or set(arguments) != set(definition):
                    raise ToolProtocolError("invalid_arguments")
                if any(type(arguments[key]) is not expected for key, expected in definition.items()):
                    raise ToolProtocolError("invalid_argument_type")
                calls.append(ToolCall(entry["id"], name, arguments))
            return StreamResult(tuple(calls), "".join(content), "".join(reasoning))
        except (UnicodeError, json.JSONDecodeError, KeyError, TypeError, AttributeError, RecursionError):
            raise ToolProtocolError("invalid_stream") from None
