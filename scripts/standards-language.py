#!/usr/bin/env python3
"""Dispatch existing product gates using the fixed operation context."""
import os
from pathlib import Path
import subprocess
import sys

ROOT = Path(__file__).resolve().parents[1]

def main(action):
    environment = dict(os.environ, RUSTUP_AUTO_INSTALL="0", CARGO_NET_OFFLINE="true")
    toolchain = environment.get("CRAFTGUILD_TOOLCHAIN", "1.95.0")
    if environment.get("CRAFTGUILD_TARGET"):
        environment["CARGO_BUILD_TARGET"] = environment["CRAFTGUILD_TARGET"]
    cargo = ["cargo", f"+{toolchain}"]
    if action == "format":
        commands = [cargo + ["fmt", "--all", "--", "--check"]]
    elif action == "lint":
        commands = [cargo + ["clippy", "--locked", "--all-targets", "--", "-D", "warnings"]]
    elif action == "test":
        commands = [cargo + ["test", "--locked"],
                    [sys.executable, "tests/smoke.py", "--binary", environment["CRAFTGUILD_OUTPUT"]]]
    else:
        raise SystemExit("Expected test, format or lint")
    for command in commands:
        subprocess.run(command, cwd=ROOT, env=environment, check=True, timeout=300)

if __name__ == "__main__":
    main(sys.argv[1])
