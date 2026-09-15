"""Disposable NSC integration and authority-boundary regression checks."""
import base64
import json
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

from jwt_server import Authority


class AccountAuthorityTests(unittest.TestCase):
    def test_real_store_and_review_boundaries(self):
        with tempfile.TemporaryDirectory() as directory:
            def nsc(*args):
                return subprocess.check_output(["nsc", "-H", directory, *args], stderr=subprocess.DEVNULL).decode()
            nsc("add", "operator", "--name", "Demo", "--sys")
            nsc("add", "account", "--name", "APP")
            claims = json.loads(nsc("describe", "account", "-n", "APP", "--json"))
            config = {"store": directory, "operator": "Demo", "account": "APP", "account_public_key": claims["sub"]}
            authority = Authority(config)
            self.assertEqual(authority.status()["identity_model"], "jwt-account")
            def preview(**operation):
                return authority.preview({"revision": authority.status()["revision"], "owner": "session-one", **operation})
            def apply(review, **overrides):
                return authority.apply({"token": review["token"], "confirmation": review["confirmation"], "owner": "session-one", **overrides})
            operation = {"action": "create_user", "name": "reader", "expiry_hours": 1, "permissions": {"publish": [], "subscribe": ["app.>"]}}
            review = preview(**operation)
            with self.assertRaises(ValueError):
                apply(review, owner="session-two")
            result = apply(review)
            self.assertIn("BEGIN NATS USER JWT", result["credentials"])
            self.assertIn("BEGIN USER NKEY SEED", result["credentials"])
            self.assertFalse(result["resolver_update_sent"])
            claims = json.loads(nsc("describe", "user", "-a", "APP", "-n", "reader", "--json"))
            self.assertEqual(claims["nats"]["pub"]["deny"], [">"])
            self.assertEqual(claims["nats"]["sub"]["allow"], ["app.>"])
            self.assertLessEqual(claims["exp"] - claims["iat"], 3600)
            self.assertNotIn("credentials", authority.status())
            with self.assertRaises(ValueError):
                apply(review)
            self.assertEqual([user["name"] for user in authority.status()["users"]], ["reader"])
            import threading
            held = preview(action="export_account")
            failures = []
            def contended():
                try:
                    apply(held)
                except BlockingIOError:
                    failures.append("busy")
            with authority.lock:
                worker = threading.Thread(target=contended)
                worker.start()
                worker.join(timeout=1)
                self.assertFalse(worker.is_alive())
                self.assertEqual(failures, ["busy"])
            self.assertIn("account_jwt", apply(held))
            old = preview(action="export_account")
            revoke = apply(preview(action="revoke_user", name="reader"))
            self.assertFalse(revoke["resolver_update_sent"])
            self.assertIn(claims["sub"], authority.status()["revocations"])
            with self.assertRaises(ValueError):
                apply(old)
            exported = apply(preview(action="export_account"))["account_jwt"]
            decoded = json.loads(base64.urlsafe_b64decode(exported.split(".")[1] + "=="))
            self.assertIn(claims["sub"], decoded["nats"]["revocations"])
            with self.assertRaises(ValueError):
                preview(action="publish_account")
            for bad in ({"name": "../../escape"}, {"expiry_hours": 0}, {"expiry_hours": True}, {"permissions": {"publish": ["a.>.b"], "subscribe": []}}, {"private_key": "seed"}):
                with self.assertRaises(ValueError):
                    preview(**{**operation, "name": "another", **bad})
            with self.assertRaises(BlockingIOError):
                Authority(config)
            metadata = Path(directory) / "Demo" / ".nsc"
            original = metadata.read_text()
            metadata.write_text(json.dumps({**json.loads(original), "managed": True}))
            with self.assertRaises(ValueError):
                authority.status()
            metadata.write_text(original)
            authority.lockfile.close()
            with self.assertRaises(ValueError):
                Authority({**config, "account_public_key": "A" + "B" * 55})

    def test_tls_resolver_and_missing_signing_authority(self):
        import socket
        import time
        with tempfile.TemporaryDirectory() as directory:
            def nsc(*args):
                return subprocess.check_output(["nsc", "-H", directory, *args], stderr=subprocess.DEVNULL, timeout=15).decode()
            nsc("add", "operator", "-n", "Demo", "--sys")
            nsc("add", "account", "-n", "APP")
            nsc("add", "user", "-a", "SYS", "-n", "publisher")
            claims = json.loads(nsc("describe", "account", "-n", "APP", "--json"))
            sys_claims = json.loads(nsc("describe", "account", "-n", "SYS", "--json"))
            cert, key = str(Path(directory) / "server.pem"), str(Path(directory) / "server.key")
            subprocess.run(["openssl", "req", "-x509", "-newkey", "rsa:2048", "-nodes", "-keyout", key, "-out", cert, "-days", "2", "-subj", "/CN=localhost", "-addext", "subjectAltName=DNS:localhost,IP:127.0.0.1", "-addext", "basicConstraints=critical,CA:TRUE"], check=True, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            with socket.socket() as listener:
                listener.bind(("127.0.0.1", 0))
                port = listener.getsockname()[1]
            url = f"nats://127.0.0.1:{port}"
            config = {"store": directory, "operator": "Demo", "account": "APP", "account_public_key": claims["sub"], "resolver_url": url, "system_user": "publisher", "ca_file": cert}
            authority = Authority(config)
            def operation(**value):
                review = authority.preview({"revision": authority.status()["revision"], **value})
                return authority.apply({"token": review["token"], "confirmation": review["confirmation"]})
            operation(action="create_user", name="client", expiry_hours=1, permissions={"publish": [], "subscribe": ["app.>"]})
            server_config = {"host": "127.0.0.1", "port": port, "operator": nsc("describe", "operator", "--raw").strip(), "system_account": sys_claims["sub"], "resolver": {"type": "full", "dir": str(Path(directory) / "resolver")}, "resolver_preload": {claims["sub"]: nsc("describe", "account", "-n", "APP", "--raw").strip(), sys_claims["sub"]: nsc("describe", "account", "-n", "SYS", "--raw").strip()}, "tls": {"cert_file": cert, "key_file": key, "ca_file": cert}}
            path = Path(directory) / "server.json"
            path.write_text(json.dumps(server_config))
            server = subprocess.Popen(["nats-server", "-c", str(path)], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
            try:
                for _ in range(50):
                    try:
                        with socket.create_connection(("127.0.0.1", port), timeout=0.1):
                            break
                    except OSError:
                        time.sleep(0.05)
                self.assertIsNone(server.poll())
                nsc("tool", "rtt", "-a", "APP", "-u", "client", "--nats", url, "--ca-cert", cert)
                operation(action="revoke_user", name="client")
                # Local revocation alone must not change the broker's trust state.
                nsc("tool", "rtt", "-a", "APP", "-u", "client", "--nats", url, "--ca-cert", cert)
                published = operation(action="publish_account")
                self.assertTrue(published["resolver_accepted"])
                with self.assertRaises(subprocess.CalledProcessError):
                    nsc("tool", "rtt", "-a", "APP", "-u", "client", "--nats", url, "--ca-cert", cert)
                seeds = {path: path.read_bytes() for path in (Path(directory) / "keys" / "O").rglob("*.nk")}
                self.assertTrue(seeds)
                for seed in seeds:
                    seed.unlink()
                operation(action="create_user", name="account-only", expiry_hours=1, permissions={"publish": [], "subscribe": ["app.>"]})
                before = authority.revision()
                with self.assertRaises(subprocess.CalledProcessError):
                    operation(action="revoke_user", name="account-only")
                self.assertEqual(before, authority.revision())
                for seed, value in seeds.items():
                    seed.write_bytes(value)
                authority.config["system_user"] = "missing"
                with self.assertRaises(subprocess.CalledProcessError):
                    operation(action="publish_account")
            finally:
                server.terminate()
                server.wait(timeout=5)
                authority.lockfile.close()


if __name__ == "__main__":
    unittest.main()
