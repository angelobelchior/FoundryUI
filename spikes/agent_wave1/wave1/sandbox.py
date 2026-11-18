import json
import math
import os
import platform
import signal
import subprocess
import tempfile
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class ProbeResult:
    status: str
    returncode: int | None = None
    stdout: str = ""
    stderr: str = ""
    product_ready: bool = False
    cleanup_verified: bool = False
    reason: str | None = None


class ProbeRunner:
    def __init__(self, backend: str):
        self.backend = backend

    def run(self, command: list[str], root: Path, timeout: float = 3) -> ProbeResult:
        if not command or any(not isinstance(argument, str) or "\0" in argument for argument in command) or not command[0].startswith("/"):
            return ProbeResult("blocked", reason="invalid_probe_command")
        if not isinstance(timeout, (int, float)) or not math.isfinite(timeout) or not 0 < timeout <= 10:
            return ProbeResult("blocked", reason="invalid_probe_timeout")
        try:
            root = root.resolve(strict=True)
            if not root.is_dir():
                return ProbeResult("blocked", reason="invalid_probe_root")
        except (OSError, RuntimeError):
            return ProbeResult("blocked", reason="invalid_probe_root")
        if self.backend != "macos" or platform.system() != "Darwin":
            return ProbeResult("unavailable", reason="backend_not_proven")
        profile = "\n".join([
            "(version 1)",
            "(deny default)",
            '(import "dyld-support.sb")',
            "(allow process-exec)",
            "(allow process-fork)",
            "(allow syscall*)",
            "(allow mach-bootstrap)",
            "(allow signal (target self))",
            "(allow sysctl-read)",
            "(allow file-read-metadata)",
            '(allow file-read* (subpath "/System/Library") (subpath "/usr/lib") (subpath "/usr/bin") (subpath "/bin") (subpath "/sbin") (literal "/dev/null") (literal "/dev/urandom") (literal "/dev/random"))',
            '(allow file-map-executable (subpath "/System/Library") (subpath "/usr/lib") (subpath "/usr/bin") (subpath "/bin"))',
            f"(allow file-read* file-write* (subpath {json.dumps(str(root))}))",
        ])
        environment = {"PATH": "/usr/bin:/bin", "HOME": "/nonexistent", "TMPDIR": str(root), "LANG": "C"}
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as errors:
            try:
                process = subprocess.Popen(["/usr/bin/sandbox-exec", "-p", profile, *command], cwd=root, env=environment, stdin=subprocess.DEVNULL, stdout=output, stderr=errors, start_new_session=True)
            except OSError:
                return ProbeResult("unavailable", reason="sandbox_launch_failed")
            status = "completed"
            try:
                process.wait(timeout=timeout)
            except subprocess.TimeoutExpired:
                status = "timed_out"
                try:
                    os.killpg(process.pid, signal.SIGKILL)
                except ProcessLookupError:
                    pass
                process.wait(timeout=2)
            output.seek(0)
            errors.seek(0)
            if status == "completed" and process.returncode != 0:
                status = "failed"
            return ProbeResult(status, process.returncode, output.read(65_536).decode(errors="replace"), errors.read(65_536).decode(errors="replace"), reason="candidate_only_tree_containment_unproven")
