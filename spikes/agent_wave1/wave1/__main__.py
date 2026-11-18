import argparse
import json
import platform
import shutil
import subprocess
from dataclasses import asdict

from wave1.model import probe_local_model


def main() -> int:
    parser = argparse.ArgumentParser(description="Wave 1 probes only. Never enables agent tools in FoundryUI.")
    commands = parser.add_subparsers(dest="command", required=True)
    commands.add_parser("environment", help="Print non-secret environment metadata without starting services")
    model = commands.add_parser("model", help="Test a model already loaded on a literal loopback endpoint")
    model.add_argument("--endpoint", required=True)
    model.add_argument("--model", required=True)
    model.add_argument("--no-think", action="store_true", help="Explicit Qwen soft switch; recorded in the result")
    arguments = parser.parse_args()
    if arguments.command == "model":
        result = probe_local_model(arguments.endpoint, arguments.model, arguments.no_think)
        print(json.dumps(asdict(result), indent=2))
        return 0 if result.status == "cycle_observed" else 2
    foundry_version = None
    executable = shutil.which("foundry")
    if executable:
        try:
            version = subprocess.run([executable, "--version"], capture_output=True, text=True, timeout=5, check=False)
            if version.returncode == 0:
                foundry_version = version.stdout.strip()[:128]
        except (OSError, subprocess.TimeoutExpired):
            pass
    print(json.dumps({
        "platform": platform.system(),
        "release": platform.release(),
        "architecture": platform.machine(),
        "python": platform.python_version(),
        "foundry": foundry_version,
        "implementation_scope": "macos_only",
        "product_ready": False,
    }, indent=2))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
