import threading
import unittest
from contextlib import contextmanager
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from socketserver import TCPServer

from wave1.network import AuditContext, MediatedClient


@contextmanager
def fixture_server(routes):
    requests = []

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            requests.append(self.path)
            status, headers, body = routes.get(self.path, (404, {}, b"missing"))
            self.send_response(status)
            for name, value in headers.items():
                self.send_header(name, value)
            self.send_header("Content-Length", str(len(body)))
            self.end_headers()
            self.wfile.write(body)

        def log_message(self, format, *args):
            pass

    class LocalFixtureServer(ThreadingHTTPServer):
        def server_bind(self):
            TCPServer.server_bind(self)
            self.server_name = "localhost"
            self.server_port = self.server_address[1]

    server = LocalFixtureServer(("127.0.0.1", 0), Handler)
    worker = threading.Thread(target=server.serve_forever, kwargs={"poll_interval": 0.02}, daemon=True)
    worker.start()
    try:
        yield f"http://127.0.0.1:{server.server_port}", requests
    finally:
        server.shutdown()
        server.server_close()
        worker.join(timeout=2)
        if worker.is_alive():
            raise RuntimeError("fixture_failed_to_stop")


class MediatedClientTests(unittest.TestCase):
    def test_allowed_request_produces_correlated_sanitized_events(self):
        with fixture_server({"/private?token=sample-secret": (200, {}, b"fixture")}) as (base, requests):
            result = MediatedClient().fetch(
                base + "/private?token=sample-secret",
                authorize=lambda url: True,
                context=AuditContext("conversation-a", "run-a", "call-a"),
            )
        self.assertEqual(result.status, "completed")
        self.assertEqual(result.http_status, 200)
        self.assertEqual(result.body, b"fixture")
        self.assertEqual(requests, ["/private?token=sample-secret"])
        self.assertEqual([event.state for event in result.events], ["requested", "attempted", "completed"])
        self.assertTrue(all(event.context.conversation_id == "conversation-a" for event in result.events))
        self.assertNotIn("sample-secret", repr(result.events))
        self.assertNotIn("/private", repr(result.events))

    def test_redirect_is_logged_and_new_destination_requires_approval(self):
        decisions = []

        def authorize(url):
            decisions.append(url)
            return len(decisions) == 1

        with fixture_server({"/start": (302, {"Location": "/secret?password=hidden"}, b""), "/secret?password=hidden": (200, {}, b"must-not-fetch")}) as (base, requests):
            result = MediatedClient().fetch(base + "/start", authorize, AuditContext("c", "r", "t"))
        self.assertEqual(result.status, "blocked")
        self.assertEqual(requests, ["/start"])
        self.assertEqual(decisions, [base + "/start", base + "/secret?password=hidden"])
        self.assertEqual([event.state for event in result.events], ["requested", "attempted", "redirected", "requested", "blocked"])
        self.assertNotIn("hidden", repr(result))

    def test_unsupported_or_credential_bearing_urls_are_blocked_before_transport(self):
        with fixture_server({"/": (200, {}, b"unexpected")}) as (base, requests):
            for url in [base.replace("http:", "ftp:"), base.replace("http://", "http://user:credential@"), base + "/\r\nsecret", "http://[bad", "file:///private/credential"]:
                with self.subTest(url=url):
                    result = MediatedClient().fetch(url, lambda value: True, AuditContext("c", "r", "t"))
                    self.assertEqual(result.status, "blocked")
                    self.assertNotIn("attempted", [event.state for event in result.events])
                    self.assertNotIn("credential", repr(result))
            self.assertEqual(requests, [])

    def test_allowed_redirect_body_limit_and_redirect_loop_are_bounded(self):
        routes = {
            "/start": (307, {"Location": "/end"}, b""),
            "/end": (200, {}, b"value"),
            "/large": (200, {}, b"x" * 20),
            "/loop": (302, {"Location": "/loop"}, b""),
        }
        with fixture_server(routes) as (base, requests):
            result = MediatedClient().fetch(base + "/start", lambda url: True, AuditContext("c", "r", "t"))
            self.assertEqual(result.status, "completed")
            self.assertEqual(result.body, b"value")
            self.assertEqual([event.state for event in result.events], ["requested", "attempted", "redirected", "requested", "attempted", "completed"])
            oversized = MediatedClient(max_body=4).fetch(base + "/large", lambda url: True, AuditContext("c", "r", "t"))
            self.assertEqual(oversized.status, "failed")
            self.assertEqual(oversized.body, b"")
            self.assertEqual(oversized.events[-1].reason, "body_limit")
            loop = MediatedClient(max_redirects=1).fetch(base + "/loop", lambda url: True, AuditContext("c", "r", "t"))
            self.assertEqual(loop.status, "failed")
            self.assertEqual(requests.count("/loop"), 2)

    def test_authorization_failure_does_not_send_request_or_leak_error(self):
        def broken_authorizer(url):
            raise RuntimeError("credential-from-approval")

        with fixture_server({}) as (base, requests):
            for authorize in [lambda url: False, lambda url: "yes", broken_authorizer]:
                result = MediatedClient().fetch(base, authorize, AuditContext("c", "r", "t"))
                self.assertEqual(result.status, "blocked")
                self.assertNotIn("credential", repr(result))
                self.assertNotIn("attempted", [event.state for event in result.events])
            self.assertEqual(requests, [])


if __name__ == "__main__":
    unittest.main()
