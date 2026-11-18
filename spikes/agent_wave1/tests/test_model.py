import json
import threading
import unittest
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from socketserver import TCPServer

from wave1.model import probe_local_model


def completion(delta, finish):
    return ("data: " + json.dumps({"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]}) + "\n\ndata: [DONE]\n\n").encode()


class LocalModelProbeTests(unittest.TestCase):
    def test_wire_fixture_confirms_call_result_and_continuation_without_file_access(self):
        captured = []

        class Handler(BaseHTTPRequestHandler):
            def do_POST(self):
                captured.append(json.loads(self.rfile.read(int(self.headers["Content-Length"]))))
                if len(captured) == 1:
                    wire = completion({"tool_calls": [{"index": 0, "id": "call-fixture", "type": "function", "function": {"name": "lookup_fixture", "arguments": '{"key":"sample"}'}}]}, "tool_calls")
                else:
                    wire = completion({"content": "fixture-value"}, "stop")
                self.send_response(200)
                self.send_header("Content-Type", "text/event-stream")
                self.send_header("Content-Length", str(len(wire)))
                self.end_headers()
                self.wfile.write(wire)

            def log_message(self, format, *args):
                pass

        class Fixture(ThreadingHTTPServer):
            def server_bind(self):
                TCPServer.server_bind(self)
                self.server_name = "localhost"
                self.server_port = self.server_address[1]

        server = Fixture(("127.0.0.1", 0), Handler)
        thread = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.02}, daemon=True)
        thread.start()
        try:
            result = probe_local_model(f"http://127.0.0.1:{server.server_port}", "fixture-model", disable_thinking=True)
        finally:
            server.shutdown()
            server.server_close()
            thread.join(timeout=2)
        self.assertEqual(result.status, "cycle_observed")
        self.assertEqual(result.model, "fixture-model")
        self.assertEqual(result.tool_calls, 1)
        self.assertTrue(captured[0]["messages"][0]["content"].endswith("/no_think"))
        self.assertTrue(result.thinking_disabled)
        self.assertEqual(captured[1]["messages"][-1], {"role": "tool", "tool_call_id": "call-fixture", "content": "fixture-value"})
        self.assertEqual(captured[1]["messages"][-2]["tool_calls"][0]["id"], "call-fixture")
        self.assertFalse(result.product_ready)

    def test_untrusted_or_invalid_endpoints_are_blocked_without_fallback(self):
        for endpoint in ["https://example.invalid", "http://localhost:5000", "http://127.0.0.1:0", "http://user:secret@127.0.0.1:5000", "http://127.0.0.1:5000/other", "http://[bad"]:
            with self.subTest(endpoint=endpoint):
                result = probe_local_model(endpoint, "fixture-model")
                self.assertEqual(result.status, "blocked")
                self.assertNotIn("secret", repr(result))


if __name__ == "__main__":
    unittest.main()
