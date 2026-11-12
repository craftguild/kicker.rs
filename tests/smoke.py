"""End-to-end smoke test using real CLI prompts, HTTP and child processes."""
import argparse
import fcntl
import hashlib
import hmac
import http.server
import json
import os
from pathlib import Path
import re
import signal
import subprocess
import tempfile
import threading
import time
import urllib.error
import urllib.request
import uuid
from interactive import register, extra_checks


def wait_for(predicate, seconds=15):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        time.sleep(0.03)
    raise AssertionError("timed out waiting for smoke-test result")


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--binary", type=Path, required=True)
    binary = parser.parse_args().binary.resolve()
    secret = "smoke-only-webhook-secret"
    auth = "Bearer smoke-only-auth-token"
    received = []

    class Receiver(http.server.BaseHTTPRequestHandler):
        def do_POST(self):
            body = self.rfile.read(int(self.headers["Content-Length"]))
            received.append((dict(self.headers), body))
            self.send_response(204)
            self.end_headers()

        def log_message(self, *args):
            pass

    receiver = http.server.ThreadingHTTPServer(("127.0.0.1", 0), Receiver)
    receiver_thread = threading.Thread(target=receiver.serve_forever, daemon=True)
    receiver_thread.start()
    try:
        with tempfile.TemporaryDirectory(prefix="kicker-smoke-") as directory:
            root = Path(directory)
            home = root / "home"
            command = "sleep 1; cat > received.json; printf '%s' \"$KICKER_EVENT\" > event"
            cli_id = register(binary, home, root, ["--call-cmd", command], secret)
            http_id = register(binary, home, root, ["--call-http", "http://127.0.0.1:%d/target" % receiver.server_port], secret, {"Authorization": auth})
            slow_id = register(binary, home, root, ["--call-cmd", "echo $$ > running.pid; exec sleep 60"], secret)
            failed_id = register(binary, home, root, ["--call-cmd", "exit 7"], secret)
            path = home / "config.toml"
            text = path.read_text().replace("shutdown_timeout_secs = 30", "shutdown_timeout_secs = 1")
            path.write_text(text)
            listing = subprocess.check_output([str(binary), "--home", str(home), "list"])
            assert cli_id.encode() in listing and http_id.encode() in listing
            assert secret.encode() not in listing and auth.encode() not in listing
            assert path.stat().st_mode & 0o777 == 0o600
            assert home.stat().st_mode & 0o777 == 0o700
            logs = root / "server.log"
            with logs.open("wb") as log:
                def launch():
                    previous = len(re.findall(r'"bind":"([^"]+)"', logs.read_text()))
                    child = subprocess.Popen([str(binary), "--home", str(home), "serve", "--bind", "127.0.0.1:0"], stdout=log, stderr=log)
                    try:
                        wait_for(lambda: len(re.findall(r'"bind":"([^"]+)"', logs.read_text())) > previous or child.poll() is not None)
                        assert child.poll() is None, "serve exited before listening"
                        return child, "http://" + re.findall(r'"bind":"([^"]+)"', logs.read_text())[-1]
                    except Exception:
                        child.terminate()
                        child.wait(timeout=10)
                        raise

                process, base = launch()
                try:
                    delivery = str(uuid.uuid4())
                    hook_prefix = "/"

                    def post(hook_id, body=b"{}", event="push", valid=True, path_prefix=None):
                        signature = hmac.new(secret.encode(), body, hashlib.sha256).hexdigest() if valid else "0" * 64
                        prefix = hook_prefix if path_prefix is None else path_prefix
                        request = urllib.request.Request(base + prefix.rstrip("/") + "/" + hook_id, data=body, method="POST", headers={
                            "Content-Type": "application/json", "X-GitHub-Event": event,
                            "X-GitHub-Delivery": delivery, "X-Hub-Signature-256": "sha256=" + signature,
                        })
                        try:
                            with urllib.request.urlopen(request, timeout=5) as response:
                                return response.status
                        except urllib.error.HTTPError as response:
                            response.close()
                            return response.code

                    raw = '{ "message": "日本語 $(touch injected)" }\r\n'.encode()
                    assert post(cli_id, raw, valid=False) == 400
                    assert post(cli_id, b"not json") == 400
                    assert post(cli_id, raw, event="ping") == 200
                    assert not (root / "received.json").exists()
                    started = time.monotonic()
                    assert post(cli_id, raw) == 202
                    assert time.monotonic() - started < 0.8, "202 waited for the command"
                    wait_for(lambda: (root / "received.json").exists() and (root / "received.json").read_bytes() == raw)
                    assert not (root / "injected").exists()
                    assert (root / "event").read_text() == "push"
                    for _ in range(2):
                        assert post(http_id, raw, "release") == 202
                    wait_for(lambda: len(received) == 2)
                    for headers, body in received:
                        headers = {key.lower(): value for key, value in headers.items()}
                        assert body == raw and headers["authorization"] == auth
                        assert headers["x-kicker-event"] == "release"
                        assert headers["x-kicker-delivery-id"] == delivery
                        assert "x-hub-signature-256" not in headers
                    path.write_text(path.read_text().replace('base_path = "/"', 'base_path = "/hooks"'))
                    updated_listing = subprocess.check_output([str(binary), "--home", str(home), "list"])
                    assert ("/hooks/" + cli_id).encode() in updated_listing
                    # CLI output follows config immediately; the process still uses root.
                    assert post(cli_id, raw) == 202
                    assert post(cli_id, raw, path_prefix="/hooks") == 404
                    hot_id = register(binary, home, root, ["--call-cmd", "cat > hot.json"], secret, base_path="/hooks")
                    assert post(hot_id, raw) == 404
                    process.send_signal(signal.SIGTERM)
                    assert process.wait(timeout=10) == 0
                    process, base = launch()
                    hook_prefix = "/hooks"
                    assert post(cli_id, raw, path_prefix="/") == 404
                    assert post(hot_id, raw) == 202
                    wait_for(lambda: (root / "hot.json").exists() and (root / "hot.json").read_bytes() == raw)
                    subprocess.check_call([str(binary), "--home", str(home), "remove", hot_id], stdout=subprocess.DEVNULL)
                    assert post(hot_id, raw) == 202
                    path.write_text(path.read_text().replace("received.json", "after-restart.json"))
                    assert post(cli_id, raw) == 202
                    assert not (root / "after-restart.json").exists()
                    process.send_signal(signal.SIGTERM)
                    assert process.wait(timeout=10) == 0
                    process, base = launch()
                    assert post(hot_id, raw) == 404
                    assert post(cli_id, raw) == 202
                    wait_for(lambda: (root / "after-restart.json").exists() and (root / "after-restart.json").read_bytes() == raw)
                    # Running admission uses no configuration-file I/O.
                    saved_config = path.read_text()
                    path.write_text("invalid = [")
                    assert post(cli_id, raw) == 202
                    path.write_text(saved_config)
                    assert post(failed_id) == 202
                    wait_for(lambda: '"exit_code":7' in logs.read_text())
                    assert post(slow_id) == 202
                    wait_for(lambda: (root / "running.pid").exists())
                    assert post(slow_id) == 202
                    # A config writer cannot block admission or shutdown of the snapshot.
                    with (home / "config.lock").open("r+b") as config_lock:
                        fcntl.flock(config_lock, fcntl.LOCK_EX)
                        assert post(cli_id, raw) == 202
                        process.send_signal(signal.SIGTERM)
                        assert process.wait(timeout=10) == 0
                    pid = int((root / "running.pid").read_text())
                    try:
                        os.kill(pid, 0)
                    except ProcessLookupError:
                        pass
                    else:
                        raise AssertionError("shutdown left the command running")
                    log_text = logs.read_text()
                    assert secret not in log_text and auth not in log_text
                    assert "日本語" not in log_text
                    events = [json.loads(line) for line in log_text.splitlines()]
                    for event in events:
                        fields = event["fields"]
                        if fields.get("message") == "job transition":
                            expected = "ERROR" if fields["state"] in ("failed", "timed_out", "retry_waiting") else "INFO"
                            assert event["level"] == expected
                    assert any(event.get("fields", {}).get("state") == "cancelled" for event in events)
                    assert all(event["timestamp"].endswith("Z") for event in events)
                    states = {}
                    transitions = set()
                    for event in events:
                        fields = event["fields"]
                        if fields.get("message") != "job transition":
                            continue
                        transition = fields["transition_id"]
                        uuid.UUID(transition)
                        assert transition not in transitions
                        transitions.add(transition)
                        job_id = fields["job_id"]
                        assert fields.get("previous_state") == states.get(job_id)
                        states[job_id] = fields["state"]
                    assert all(state in ("succeeded", "failed", "cancelled") for state in states.values())
                finally:
                    if process.poll() is None:
                        process.terminate()
                        try:
                            process.wait(timeout=10)
                        except subprocess.TimeoutExpired:
                            process.kill()
                            process.wait()
            extra_checks(binary, root)
        print("smoke: interactive setup, signatures, stdin, HTTP auth, redelivery, configurable paths, config-based CLI output, restart-only configuration, transition logs and shutdown with retry/pending jobs under config-lock contention passed")
    finally:
        receiver.shutdown()
        receiver.server_close()
        receiver_thread.join()


if __name__ == "__main__":
    main()
