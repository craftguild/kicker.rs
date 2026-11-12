"""Real terminal checks; fixtures and secrets exist only in temporary directories."""
import errno
import os
import pty
import re
import select
import signal
import time
from pathlib import Path


class Terminal:
    def __init__(self, binary, home, args, cwd):
        pid, master = pty.fork()
        if pid == 0:
            os.chdir(cwd)
            os.execv(str(binary), [str(binary), "--home", str(home)] + args)
        self.pid, self.master = pid, master
        self.output = b""
        self.cursor = 0
        self.finished = False

    def read(self):
        if select.select([self.master], [], [], 0.1)[0]:
            try:
                self.output += os.read(self.master, 65536)
            except OSError as error:
                if error.errno != errno.EIO:
                    raise

    def expect(self, text):
        pattern = text.encode()
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            index = self.output.find(pattern, self.cursor)
            if index >= 0:
                self.cursor = index + len(pattern)
                return
            self.read()
        raise AssertionError("terminal prompt missing: " + text)

    def send(self, value):
        time.sleep(0.04)
        os.write(self.master, value.encode())

    def wait(self, success=True):
        deadline = time.monotonic() + 15
        while time.monotonic() < deadline:
            self.read()
            pid, status = os.waitpid(self.pid, os.WNOHANG)
            if pid:
                self.finished = True
                for _ in range(3):
                    self.read()
                code = os.waitstatus_to_exitcode(status)
                assert (code == 0) == success, "unexpected terminal command exit"
                return self.output
        raise AssertionError("terminal command did not exit")

    def close(self):
        if not self.finished:
            os.kill(self.pid, signal.SIGKILL)
            os.waitpid(self.pid, 0)
        os.close(self.master)


def register(binary, home, cwd, target_args, secret, headers=None, base_path="/"):
    terminal = Terminal(binary, home, ["new", "--type", "github"] + target_args, cwd)
    try:
        terminal.expect("GitHub webhook secret")
        terminal.send("\x1b[B\r")
        terminal.expect("Secret")
        terminal.send(secret + "\r")
        terminal.expect("Repeat secret")
        terminal.send(secret + "\r")
        if "--call-http" in target_args:
            for name, value in (headers or {}).items():
                terminal.expect("Add an HTTP header?")
                terminal.send("y\r")
                terminal.expect("Header name")
                terminal.send(name + "\r")
                terminal.expect("Header value (hidden)")
                terminal.send(value + "\r")
            terminal.expect("Add an HTTP header?")
            terminal.send("n\r")
        output = terminal.wait()
        assert secret.encode() not in output, "entered secret was echoed"
        for value in (headers or {}).values():
            assert value.encode() not in output, "header value was echoed"
        hook_id = re.search(rb"Registered ([0-9a-f-]{36})", output).group(1).decode()
        expected_path = base_path.rstrip("/") + "/" + hook_id
        assert ("Webhook path: " + expected_path).encode() in output
        return hook_id
    finally:
        terminal.close()


def extra_checks(binary, root):
    home = Path(root) / "generated-home"
    terminal = Terminal(binary, home, ["new", "--type", "github", "--call-cmd", "true"], root)
    try:
        terminal.expect("GitHub webhook secret")
        terminal.send("\r")
        terminal.expect("Secret copied?")
        terminal.send("y\r")
        output = terminal.wait()
        secret = re.search(rb"[0-9a-f]{64}", output).group().decode()
        assert secret in (home / "config.toml").read_text()
        assert output.count(secret.encode()) == 1
    finally:
        terminal.close()
    cancelled = Path(root) / "cancelled-home"
    terminal = Terminal(binary, cancelled, ["new", "--type", "github", "--call-cmd", "true"], root)
    try:
        terminal.expect("GitHub webhook secret")
        terminal.send("\x03")
        terminal.wait(success=False)
        assert not cancelled.exists(), "cancelled setup wrote configuration"
    finally:
        terminal.close()
