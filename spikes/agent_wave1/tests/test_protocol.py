import json
import unittest

from wave1.protocol import ToolProtocolError, ToolStream


def event(payload):
    return ("data: " + json.dumps(payload, ensure_ascii=False) + "\n\n").encode()


class ToolStreamTests(unittest.TestCase):
    def test_fragmented_call_is_available_only_after_complete_stream(self):
        stream = ToolStream({"lookup_fixture": {"key": str}})
        chunks = [
            event({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "id": "call_a", "type": "function", "function": {
                    "name": "lookup_fixture", "arguments": '{"key":'
                }}
            ]}}]}),
            event({"choices": [{"index": 0, "delta": {"tool_calls": [
                {"index": 0, "id": None, "type": None, "function": {"name": None, "arguments": '"sample"}'}}
            ]}, "finish_reason": "tool_calls"}]}),
            b"data: [DONE]\n\n",
        ]
        for chunk in b"".join(chunks):
            self.assertIsNone(stream.feed(bytes([chunk])))
        result = stream.finish()
        self.assertEqual(len(result.calls), 1)
        self.assertEqual(result.calls[0].id, "call_a")
        self.assertEqual(result.calls[0].name, "lookup_fixture")
        self.assertEqual(result.calls[0].arguments, {"key": "sample"})

    def test_invalid_batch_never_exposes_executable_calls(self):
        good = {"index": 0, "id": "call_a", "type": "function", "function": {
            "name": "lookup_fixture", "arguments": '{"key":"sample"}'
        }}
        cases = {
            "missing_done": event({"choices": [{"delta": {"tool_calls": [good]}, "finish_reason": "tool_calls"}]}),
            "truncated": event({"choices": [{"delta": {"tool_calls": [good]}, "finish_reason": "length"}]}) + b"data: [DONE]\n\n",
            "duplicate_id": event({"choices": [{"delta": {"tool_calls": [good, dict(good, index=1)]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n",
            "after_done": event({"choices": [{"delta": {}, "finish_reason": "stop"}]}) + b"data: [DONE]\n\n" + event({"choices": [{"delta": {"tool_calls": [good]}, "finish_reason": "tool_calls"}]}),
            "wrong_type": event({"choices": [{"delta": {"tool_calls": [dict(good, type="other")]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n",
            "duplicate_argument": event({"choices": [{"delta": {"tool_calls": [dict(good, function={"name": "lookup_fixture", "arguments": '{"key":"a","key":"b"}'})]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n",
            "empty_tool_finish": event({"choices": [{"delta": {}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n",
        }
        for label, wire in cases.items():
            with self.subTest(case=label):
                stream = ToolStream({"lookup_fixture": {"key": str}})
                stream.feed(wire)
                with self.assertRaises(ToolProtocolError):
                    stream.finish()

    def test_crlf_unicode_interleaving_and_untrusted_text_stay_separate(self):
        parts = [
            {"choices": [{"delta": {"content": "```sh\nrm -rf /\n``` ação", "reasoning_content": '<|call|>{"name":"shell"}'}}]},
            {"choices": [{"delta": {"tool_calls": [
                {"index": 1, "id": "call_b", "function": {"name": "lookup_", "arguments": '{"key":"b'}},
                {"index": 0, "id": "call_a", "function": {"name": "lookup_fixture", "arguments": '{"key":"a"}'}},
            ]}}]},
            {"choices": [{"delta": {"tool_calls": [
                {"index": 1, "function": {"name": "fixture", "arguments": '"}'}},
            ]}, "finish_reason": "tool_calls"}]},
            {"choices": [], "usage": {"total_tokens": 30}},
        ]
        wire = b": keepalive\r\n\r\n" + b"".join(event(part).replace(b"\n", b"\r\n") for part in parts) + b"data: [DONE]\r\n\r\n"
        stream = ToolStream({"lookup_fixture": {"key": str}})
        for offset in range(0, len(wire), 3):
            stream.feed(wire[offset:offset + 3])
        result = stream.finish()
        self.assertEqual([call.id for call in result.calls], ["call_a", "call_b"])
        self.assertEqual([call.arguments for call in result.calls], [{"key": "a"}, {"key": "b"}])
        self.assertIn("ação", result.content)
        self.assertIn("<|call|>", result.reasoning)

    def test_resource_limits_and_invalid_shapes_fail_closed(self):
        invalid_parts = [
            {"index": -1, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":"a"}'}},
            {"index": True, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":"a"}'}},
            {"index": 16, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":"a"}'}},
            {"index": 0, "id": 12, "function": {"name": "lookup_fixture", "arguments": '{"key":"a"}'}},
            {"index": 0, "id": "a", "function": {"name": "unknown", "arguments": '{"key":"a"}'}},
            {"index": 0, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":true}'}},
            {"index": 0, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":"a","extra":"b"}'}},
        ]
        for part in invalid_parts:
            with self.subTest(part=part):
                stream = ToolStream({"lookup_fixture": {"key": str}})
                stream.feed(event({"choices": [{"delta": {"tool_calls": [part]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n")
                with self.assertRaises(ToolProtocolError):
                    stream.finish()
        stream = ToolStream({"lookup_fixture": {"key": str}})
        with self.assertRaises(ToolProtocolError):
            stream.feed(b"x" * (1_048_576 + 1))

    def test_nonstandard_json_numbers_and_unfinished_event_are_rejected(self):
        wire = event({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "a", "function": {"name": "numeric", "arguments": '{"value":NaN}'}}]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n"
        stream = ToolStream({"numeric": {"value": float}})
        stream.feed(wire)
        with self.assertRaises(ToolProtocolError):
            stream.finish()
        stream = ToolStream({})
        stream.feed(event({"choices": [{"delta": {"content": "text"}, "finish_reason": "stop"}]}) + b"data: [DONE]")
        with self.assertRaises(ToolProtocolError):
            stream.finish()

    def test_limit_error_permanently_invalidates_the_stream(self):
        stream = ToolStream({})
        stream.feed(event({"choices": [{"delta": {"content": "safe"}, "finish_reason": "stop"}]}) + b"data: [DONE]\n\n")
        with self.assertRaises(ToolProtocolError):
            stream.feed(b"x" * 1_048_576)
        with self.assertRaises(ToolProtocolError):
            stream.finish()

    def test_numeric_exponent_overflow_cannot_return_nonfinite_arguments(self):
        for value in ["1e999", "-1e999"]:
            with self.subTest(value=value):
                wire = event({"choices": [{"delta": {"tool_calls": [{"index": 0, "id": "a", "function": {"name": "numeric", "arguments": '{"value":' + value + '}'}}]}, "finish_reason": "tool_calls"}]}) + b"data: [DONE]\n\n"
                stream = ToolStream({"numeric": {"value": float}})
                stream.feed(wire)
                with self.assertRaises(ToolProtocolError):
                    stream.finish()

    def test_provider_error_or_invalid_envelope_invalidates_prior_calls(self):
        call = {"index": 0, "id": "a", "function": {"name": "lookup_fixture", "arguments": '{"key":"sample"}'}}
        prefix = event({"choices": [{"delta": {"tool_calls": [call]}, "finish_reason": "tool_calls"}]})
        for invalid in [{"error": {"message": "private-provider-error", "type": "server_error"}}, {}, {"choices": []}, {"choices": None}, []]:
            with self.subTest(payload=invalid):
                stream = ToolStream({"lookup_fixture": {"key": str}})
                stream.feed(prefix + event(invalid) + b"data: [DONE]\n\n")
                with self.assertRaises(ToolProtocolError) as raised:
                    stream.finish()
                self.assertNotIn("private-provider-error", str(raised.exception))


if __name__ == "__main__":
    unittest.main()
