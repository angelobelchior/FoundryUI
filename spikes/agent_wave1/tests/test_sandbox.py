import os
import platform
import shlex
import socket
import tempfile
import threading
import unittest
from pathlib import Path

from wave1.sandbox import ProbeRunner


class SandboxContractTests(unittest.TestCase):
    def test_unimplemented_or_wrong_host_backend_never_falls_back_to_shell(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            for backend in ["windows", "unknown", "linux-native"]:
                result = ProbeRunner(backend).run(["/bin/sh", "-c", "touch escaped"], root)
                self.assertEqual(result.status, "unavailable")
                self.assertFalse(result.product_ready)
                self.assertFalse((root / "escaped").exists())

    def test_invalid_probe_request_is_blocked_before_launch(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            cases = [([], root, 1), (["sh"], root, 1), (["/bin/sh", "bad\0argument"], root, 1), (["/bin/sh"], root, 0), (["/bin/sh"], root, float("nan")), (["/bin/sh"], root / "missing", 1)]
            for command, path, timeout in cases:
                with self.subTest(command=command, path=path, timeout=timeout):
                    result = ProbeRunner("macos").run(command, path, timeout)
                    self.assertEqual(result.status, "blocked")
                    self.assertFalse(result.product_ready)


@unittest.skipUnless("macos" in os.environ.get("WAVE1_SANDBOX_BACKENDS", "").split(","), "macOS probe is opt-in")
class MacSandboxTests(unittest.TestCase):
    def test_allowed_file_operations_run_inside_selected_fixture(self):
        self.assertEqual(platform.system(), "Darwin")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            (root / "input").write_text("fixture")
            result = ProbeRunner("macos").run(["/bin/sh", "-c", 'cat input; printf changed > output'], root)
            self.assertEqual(result.status, "completed", result)
            self.assertEqual(result.returncode, 0, result)
            self.assertEqual(result.stdout, "fixture")
            self.assertEqual((root / "output").read_text(), "changed")
            self.assertFalse(result.product_ready)

    def test_outside_files_and_symlinks_are_denied_and_credentials_not_inherited(self):
        with tempfile.TemporaryDirectory() as directory:
            parent = Path(directory).resolve()
            root = parent / "allowed"
            root.mkdir()
            outside = parent / "outside"
            outside.write_text("private-fixture")
            (root / "escape").symlink_to(outside)
            runner = ProbeRunner("macos")
            for command in [f"cat {shlex.quote(str(outside))}", "cat escape", f"printf damaged > {shlex.quote(str(outside))}", f"ln -s {shlex.quote(str(outside))} swapped; cat swapped"]:
                result = runner.run(["/bin/sh", "-c", command], root)
                self.assertNotEqual(result.returncode, 0, result)
                self.assertNotIn("private-fixture", result.stdout)
                self.assertEqual(outside.read_text(), "private-fixture")
            previous = os.environ.get("WAVE1_PARENT_SECRET")
            os.environ["WAVE1_PARENT_SECRET"] = "fixture-credential"
            try:
                result = runner.run(["/bin/sh", "-c", "env"], root)
            finally:
                if previous is None:
                    os.environ.pop("WAVE1_PARENT_SECRET", None)
                else:
                    os.environ["WAVE1_PARENT_SECRET"] = previous
            self.assertEqual(result.returncode, 0, result)
            self.assertNotIn("fixture-credential", result.stdout)
            self.assertNotIn("WAVE1_PARENT_SECRET", result.stdout)

    def test_tcp_and_unix_socket_connections_are_denied(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            for family, address in [(socket.AF_INET, ("127.0.0.1", 0)), (socket.AF_UNIX, str(root / "s"))]:
                with self.subTest(family=family), socket.socket(family) as listener:
                    listener.bind(address)
                    listener.listen(4)
                    target = listener.getsockname()
                    with socket.socket(family) as control:
                        control.connect(target)
                        accepted, _ = listener.accept()
                        accepted.close()
                    if family == socket.AF_INET:
                        command = ["/usr/bin/curl", "--disable", "--noproxy", "*", "--max-time", "1", "-sS", f"http://127.0.0.1:{target[1]}"]
                    else:
                        command = ["/usr/bin/curl", "--disable", "--noproxy", "*", "--max-time", "1", "-sS", "--unix-socket", str(target), "http://localhost/"]
                    result = ProbeRunner("macos").run(command, root)
                    self.assertNotEqual(result.returncode, 0, result)
                    listener.settimeout(0.1)
                    with self.assertRaises(TimeoutError):
                        connection, _ = listener.accept()
                        connection.close()

    def test_detached_descendant_demonstrates_group_kill_is_not_containment(self):
        self.assertTrue(Path("/usr/bin/perl").is_file(), "This negative proof requires the system Perl")
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory).resolve()
            program = 'if(fork()==0){POSIX::setsid();select(undef,undef,undef,0.4);open(my $f,">","detached-marker") or die;print $f "escaped-group";close($f);exit 0;} sleep 2;'
            result = ProbeRunner("macos").run(["/usr/bin/perl", "-MPOSIX", "-e", program], root, timeout=0.1)
            threading.Event().wait(0.7)
            self.assertEqual(result.status, "timed_out", result)
            self.assertFalse(result.cleanup_verified)
            self.assertFalse(result.product_ready)
            self.assertEqual((root / "detached-marker").read_text(), "escaped-group")


if __name__ == "__main__":
    unittest.main()
