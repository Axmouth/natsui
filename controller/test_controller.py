"""Disposable process and HTTPS checks for the managed deployment adapter."""

import json
import os
from pathlib import Path
import secrets
import signal
import socket
import ssl
import subprocess
import tempfile
import time
import unittest
import urllib.error
import urllib.request

import bcrypt


def port():
    with socket.socket() as listener:
        listener.bind(("127.0.0.1", 0))
        return listener.getsockname()[1]


class ControllerTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory(prefix="natsui-controller-")
        self.addCleanup(self.temporary.cleanup)
        self.directory = Path(self.temporary.name)
        self.client_port, self.monitor_port = port(), port()
        self.token = secrets.token_hex(32)
        (self.directory / "token").write_text(self.token)
        self.password = secrets.token_hex(32)
        base = {"server_name": "controller-test", "listen": f"127.0.0.1:{self.client_port}", "http": f"127.0.0.1:{self.monitor_port}",
                "jetstream": {"store_dir": str(self.directory / "jetstream")},
                "authorization": {"users": [{"user": "bootstrap", "password": bcrypt.hashpw(self.password.encode(), bcrypt.gensalt()).decode()}]}}
        (self.directory / "base.json").write_text(json.dumps(base))
        subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", str(self.directory / "server.key"), "-out", str(self.directory / "server.pem"), "-days", "1", "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1"], check=True, capture_output=True)
        self.context = ssl.create_default_context(cafile=str(self.directory / "server.pem"))
        env = {**os.environ, "NATSUI_CONTROL_BASE_FILE": str(self.directory / "base.json"), "NATSUI_CONTROL_TOKEN_FILE": str(self.directory / "token"),
               "NATSUI_CONTROL_CERT_FILE": str(self.directory / "server.pem"), "NATSUI_CONTROL_KEY_FILE": str(self.directory / "server.key"), "NATSUI_CONTROL_DATA": str(self.directory / "state")}
        self.logs = open(self.directory / "process.log", "wb")
        self.addCleanup(self.logs.close)
        self.process = subprocess.Popen(["python3", str(Path(__file__).with_name("server.py"))], env=env, stdout=self.logs, stderr=self.logs)
        self.addCleanup(self.stop_process)
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            try:
                if self.request("status")[1].get("observed"):
                    return
            except (OSError, urllib.error.URLError):
                pass
            time.sleep(0.1)
        self.fail("Controller did not become ready")

    def stop_process(self):
        if self.process.poll() is None:
            self.process.terminate()
            self.process.wait(timeout=20)

    def request(self, path, body=None, token=True, timeout=5):
        headers = {"Content-Type": "application/json"}
        if token:
            headers["Authorization"] = "Bearer " + self.token
        request = urllib.request.Request("https://127.0.0.1:9443/v1/" + path, data=json.dumps(body).encode() if body is not None else None, headers=headers)
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), urllib.request.HTTPSHandler(context=self.context))
        try:
            with opener.open(request, timeout=timeout) as response:
                return response.status, json.load(response)
        except urllib.error.HTTPError as error:
            return error.code, json.load(error)

    def proposal(self, body):
        body["revision"] = self.request("status")[1]["revision"]
        status, result = self.request("preview", body)
        self.assertEqual(status, 200)
        return result

    def apply(self, preview):
        status, result = self.request("apply", {"token": preview["token"], "confirmation": "APPLY"})
        self.assertEqual(status, 200)
        self.assertTrue(result["reload_or_restart_observed"])
        return result

    def read_reply(self, client):
        reply = b""
        while b"PONG" not in reply and b"-ERR" not in reply:
            chunk = client.recv(8192)
            if not chunk:
                break
            reply += chunk
        return reply

    def login(self, username, password, publish=None):
        with socket.create_connection(("127.0.0.1", self.client_port), timeout=3) as client:
            client.settimeout(3)
            client.recv(8192)
            client.sendall(("CONNECT " + json.dumps({"user": username, "pass": password}) + "\r\nPING\r\n").encode())
            reply = self.read_reply(client)
            if b"PONG" not in reply:
                return False
            if publish:
                client.sendall((f"PUB {publish} 1\r\nx\r\nPING\r\n").encode())
                reply = self.read_reply(client)
                return b"Permissions Violation" not in reply and b"PONG" in reply
            return True

    def test_oversized_candidate_cannot_replace_readable_configuration(self):
        from server import Controller
        control = Controller(self.directory / "candidate-check", self.directory / "base.json")
        before = control.config.read_bytes()
        candidate = json.loads(before)
        candidate["server_name"] = "x" * 1048576
        with self.assertRaisesRegex(ValueError, "exceeds 1 MiB"):
            control.validate(candidate)
        self.assertEqual(control.config.read_bytes(), before)

    def test_https_requires_token_and_silent_peer_does_not_block(self):
        self.assertEqual(self.request("status", token=False)[0], 401)
        with socket.create_connection(("127.0.0.1", 9443), timeout=2):
            self.assertEqual(self.request("status", timeout=2)[0], 200)

    def test_reload_restart_user_rotation_and_validation(self):
        original = self.request("status")[1]
        bad = {"revision": original["revision"], "action": "settings", "settings": {"max_payload": 0}}
        self.assertEqual(self.request("preview", bad)[0], 409)
        self.assertEqual(self.request("status")[1]["revision"], original["revision"])
        preview = self.proposal({"action": "settings", "settings": {"max_payload": 2097152}})
        self.assertEqual(self.request("apply", {"token": preview["token"], "confirmation": "WRONG"})[0], 409)
        self.apply(preview)
        self.assertEqual(self.request("apply", {"token": preview["token"], "confirmation": "APPLY"})[0], 409)
        self.assertEqual(self.request("status")[1]["observed"]["max_payload"], 2097152)
        result = self.apply(self.proposal({"action": "create_user", "name": "reader", "permissions": {"publish": [], "subscribe": ["app.>"]}}))
        self.assertTrue(self.login("reader", result["password"]))
        self.assertFalse(self.login("reader", result["password"], "app.denied"))
        persisted = (self.directory / "state/nats.json").read_text()
        self.assertNotIn(result["password"], persisted)
        rotated = self.apply(self.proposal({"action": "rotate_user", "name": "reader"}))
        self.assertFalse(self.login("reader", result["password"]))
        self.assertTrue(self.login("reader", rotated["password"]))
        self.apply(self.proposal({"action": "create_user", "name": "peer", "password": rotated["password"], "permissions": {"publish": ["app.>"], "subscribe": ["app.>"]}}))
        self.assertTrue(self.login("peer", rotated["password"]))
        self.apply(self.proposal({"action": "restart"}))
        self.assertTrue(self.login("reader", rotated["password"]))
        self.apply(self.proposal({"action": "delete_user", "name": "reader"}))
        self.apply(self.proposal({"action": "delete_user", "name": "peer"}))
        current = self.request("status")[1]
        self.assertEqual(self.request("preview", {"revision": current["revision"], "action": "delete_user", "name": "bootstrap"})[0], 409)

    def test_reviews_are_independent_and_bound_to_a_session(self):
        first = self.proposal({"action": "settings", "settings": {"max_payload": 2097152}, "owner": "alice"})
        second = self.proposal({"action": "settings", "settings": {"max_payload": 3145728}, "owner": "bob"})
        wrong_owner = {"token": first["token"], "confirmation": "APPLY", "owner": "bob"}
        self.assertEqual(self.request("apply", wrong_owner)[0], 409)
        correct = {**wrong_owner, "owner": "alice"}
        self.assertEqual(self.request("apply", correct)[0], 200)
        self.assertEqual(self.request("apply", correct)[0], 409)
        self.assertEqual(self.request("apply", {"token": second["token"], "confirmation": "APPLY", "owner": "bob"})[0], 409)

    def test_unexpected_nats_exit_terminates_controller(self):
        children = Path(f"/proc/{self.process.pid}/task/{self.process.pid}/children").read_text().split()
        self.assertEqual(len(children), 1)
        os.kill(int(children[0]), signal.SIGKILL)
        self.assertNotEqual(self.process.wait(timeout=5), 0)


if __name__ == "__main__":
    unittest.main()
