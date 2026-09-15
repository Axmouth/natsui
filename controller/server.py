"""Optional authority over one NATS process and its JSON configuration."""

import copy
import hashlib
import hmac
import http.server
import json
import os
from pathlib import Path
import re
import secrets
import signal
import ssl
import subprocess
import threading
import time
import urllib.request

import bcrypt

LIMITS = {"max_connections": (1, 1000000), "max_payload": (1024, 67108864), "max_subscriptions": (1, 1000000)}


def encoded(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def revision(value):
    return hashlib.sha256(encoded(value)).hexdigest()


def atomic(path, value):
    staged = path.with_suffix(".tmp")
    with open(staged, "wb") as output:
        os.chmod(staged, 0o600)
        output.write(encoded(value))
        output.flush()
        os.fsync(output.fileno())
    os.replace(staged, path)
    descriptor = os.open(path.parent, os.O_RDONLY)
    try:
        os.fsync(descriptor)
    finally:
        os.close(descriptor)


def read_json(path):
    with open(path, "rb") as source:
        data = source.read(1048577)
    if len(data) > 1048576:
        raise ValueError("Configuration exceeds 1 MiB")
    return json.loads(data)


def permissions(value):
    if not isinstance(value, dict) or set(value) != {"publish", "subscribe"}:
        raise ValueError("Explicit publish and subscribe subject lists are required")
    result = {}
    for kind, subjects in value.items():
        if not isinstance(subjects, list) or len(subjects) > 100:
            raise ValueError("Subject lists are limited to 100 entries")
        for subject in subjects:
            if not isinstance(subject, str) or not subject or len(subject) > 1024 or re.search(r"\s", subject):
                raise ValueError("Invalid permission subject")
            tokens = subject.split(".")
            if any(not t or (t == ">" and i != len(tokens) - 1) or (t not in ("*", ">") and ("*" in t or ">" in t)) for i, t in enumerate(tokens)):
                raise ValueError("Invalid permission wildcard")
        result[kind] = {"allow": subjects} if subjects else {"deny": [">"]}
    return result


class Controller:
    def __init__(self, directory, base, binary="/usr/local/bin/nats-server"):
        self.directory = Path(directory)
        self.directory.mkdir(parents=True, exist_ok=True)
        self.config = self.directory / "nats.json"
        self.binary = binary
        self.lock = threading.RLock()
        self.pending = {}
        self.child = None
        if not self.config.exists():
            initial = read_json(base)
            if "operator" in initial or "accounts" in initial or not initial.get("authorization", {}).get("users"):
                raise ValueError("This adapter requires config-based authorization.users with at least one user")
            self.validate(initial)
            atomic(self.config, initial)
        self.validate(read_json(self.config))

    def validate(self, config):
        if len(encoded(config)) > 1048576:
            raise ValueError("Configuration exceeds 1 MiB")
        users = config.get("authorization", {}).get("users", [])
        if "accounts" in config or "operator" in config or not users or any(not isinstance(u.get("user"), str) or not isinstance(u.get("password"), str) for u in users):
            raise ValueError("Only config-based username and password identities are supported")
        path = self.directory / "validate.json"
        atomic(path, config)
        try:
            result = subprocess.run([self.binary, "-t", "-c", str(path)], capture_output=True, timeout=10)
            if result.returncode:
                raise ValueError("NATS rejected the candidate configuration")
        finally:
            path.unlink(missing_ok=True)

    def start(self):
        self.child = subprocess.Popen([self.binary, "-c", str(self.config)])

    def stop(self):
        if self.child and self.child.poll() is None:
            self.child.terminate()
            try:
                self.child.wait(timeout=15)
            except subprocess.TimeoutExpired:
                self.child.kill()
                self.child.wait(timeout=5)

    def observe(self):
        config = read_json(self.config)
        monitoring = config.get("http", config.get("http_port", 8222))
        try:
            port = int(str(monitoring).rsplit(":", 1)[-1])
            opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
            with opener.open(f"http://127.0.0.1:{port}/varz", timeout=1) as response:
                data = response.read(2000001)
                if len(data) > 2000000:
                    return None
                raw = json.loads(data)
                return {key: raw.get(key) for key in ["server_id", "start", "config_load_time", "max_connections", "max_payload", "max_subscriptions"]}
        except (OSError, ValueError):
            return None

    def status(self):
        with self.lock:
            config = read_json(self.config)
            return {"revision": revision(config), "running": self.child is not None and self.child.poll() is None,
                    "settings": {key: config.get(key) for key in LIMITS}, "observed": self.observe(),
                    "users": [{"name": user["user"], "permissions": user.get("permissions", {})} for user in config["authorization"]["users"]],
                    "identity_model": "config-based authorization.users", "restart_required_fields": ["max_subscriptions"]}

    def preview(self, request):
        with self.lock:
            if set(request) - {"revision", "action", "settings", "name", "permissions", "password", "owner"}:
                raise ValueError("Unsupported operation field")
            self.pending = {token: entry for token, entry in self.pending.items() if entry["expires"] > time.monotonic()}
            if len(self.pending) >= 32:
                raise ValueError("Review limit reached")
            owner = request.get("owner", "direct")
            if not isinstance(owner, str) or len(owner) > 128:
                raise ValueError("Invalid review owner")
            config = read_json(self.config)
            if request.get("revision") != revision(config):
                raise ValueError("Configuration changed. Reload before reviewing")
            candidate = copy.deepcopy(config)
            action = request.get("action")
            name = request.get("name", "")
            password = None
            restart = action == "restart"
            if action == "settings":
                settings = request.get("settings")
                if not isinstance(settings, dict) or not settings or set(settings) - set(LIMITS):
                    raise ValueError("Unsupported server setting")
                for key, value in settings.items():
                    minimum, maximum = LIMITS[key]
                    if type(value) is not int or not minimum <= value <= maximum:
                        raise ValueError("Server limit is outside the supported range")
                    candidate[key] = value
                restart = "max_subscriptions" in settings and settings["max_subscriptions"] != config.get("max_subscriptions")
            elif action in ("create_user", "rotate_user", "permissions", "delete_user"):
                if not re.fullmatch(r"[a-zA-Z0-9_-]{1,48}", name):
                    raise ValueError("Invalid NATS user name")
                users = candidate["authorization"]["users"]
                user = next((user for user in users if user.get("user") == name), None)
                if action == "create_user":
                    if user or len(users) >= 256:
                        raise ValueError("User exists or the 256-user limit was reached")
                    user = {"user": name}
                    users.append(user)
                elif user is None:
                    raise ValueError("User does not exist")
                if action == "delete_user":
                    if len(users) <= 1:
                        raise ValueError("The last NATS user cannot be deleted")
                    users.remove(user)
                if action in ("create_user", "rotate_user"):
                    password = request.get("password") or secrets.token_hex(32)
                    if not isinstance(password, str) or not re.fullmatch(r"[a-fA-F0-9]{64}", password):
                        raise ValueError("A supplied password must be a generated 64-character hexadecimal credential")
                    user["password"] = bcrypt.hashpw(password.encode(), bcrypt.gensalt(rounds=12)).decode()
                if action in ("create_user", "permissions"):
                    user["permissions"] = permissions(request.get("permissions"))
            elif action != "restart":
                raise ValueError("Unsupported managed operation")
            self.validate(candidate)
            token = secrets.token_hex(32)
            self.pending[token] = {"token": token, "owner": owner, "expires": time.monotonic() + 60, "before": revision(config), "config": candidate, "password": password, "restart": restart, "name": name, "action": action}
            return {"token": token, "expires_in": 60, "action": action, "name": name, "restart": restart,
                    "settings": request.get("settings"), "permissions": request.get("permissions"),
                    "warning": "Restart disconnects clients and can affect quorum. Apply to one node at a time." if restart else "Live reload can revoke client access. Apply matching user changes to each configured cluster node."}

    def apply(self, request):
        with self.lock:
            token = str(request.get("token", ""))
            pending = self.pending.get(token)
            if not pending or pending["owner"] != request.get("owner", "direct") or request.get("confirmation") != "APPLY" or not hmac.compare_digest(str(request.get("token", "")), pending["token"]) or pending["expires"] < time.monotonic():
                raise ValueError("Preview expired or confirmation did not match")
            del self.pending[token]
            before = read_json(self.config)
            if revision(before) != pending["before"]:
                raise ValueError("Configuration changed. No operation was applied")
            self.validate(pending["config"])
            observed_before = self.observe()
            atomic(self.directory / "previous.json", before)
            atomic(self.config, pending["config"])
            if pending["restart"]:
                self.stop()
                self.start()
            elif self.child is None or self.child.poll() is not None:
                raise ValueError("Configuration saved but NATS is stopped. Explicit restart is required")
            else:
                self.child.send_signal(signal.SIGHUP)
            verified = False
            observed = None
            for _ in range(20):
                time.sleep(0.1)
                observed = self.observe()
                if observed and observed_before:
                    field = "start" if pending["restart"] else "config_load_time"
                    if observed.get(field) and observed[field] != observed_before.get(field):
                        verified = True
                        break
            return {"saved": True, "reload_or_restart_observed": verified, "observed": observed,
                    "password": pending["password"], "name": pending["name"], "revision": revision(pending["config"])}


class Server(http.server.ThreadingHTTPServer):
    daemon_threads = True

    def __init__(self, *args):
        self.workers = threading.BoundedSemaphore(16)
        super().__init__(*args)

    def process_request(self, request, address):
        if not self.workers.acquire(blocking=False):
            self.shutdown_request(request)
            return
        try:
            super().process_request(request, address)
        except BaseException:
            self.workers.release()
            raise

    def process_request_thread(self, request, address):
        try:
            super().process_request_thread(request, address)
        finally:
            self.workers.release()


class Handler(http.server.BaseHTTPRequestHandler):
    def log_message(self, *_):
        pass

    def setup(self):
        super().setup()
        self.connection.settimeout(10)

    def send(self, code, value):
        body = encoded(value)
        self.send_response(code)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(body)

    def authorized(self):
        return hmac.compare_digest(self.headers.get("Authorization", ""), "Bearer " + self.server.token)

    def do_GET(self):
        if not self.authorized():
            return self.send(401, {"error": "Authentication required"})
        if self.path != "/v1/status":
            return self.send(404, {"error": "Unknown operation"})
        try:
            self.send(200, self.server.controller.status())
        except (ValueError, TypeError, OSError, subprocess.SubprocessError):
            self.send(503, {"error": "Controller status unavailable. Inspect the protected store and configured authority."})

    def do_POST(self):
        if not self.authorized():
            return self.send(401, {"error": "Authentication required"})
        try:
            length = int(self.headers.get("Content-Length", "0"))
            if not 0 < length <= 65536:
                return self.send(413, {"error": "Request limit is 64 KiB"})
            request = json.loads(self.rfile.read(length))
            if not isinstance(request, dict):
                raise ValueError("Expected an operation object")
            if self.path == "/v1/preview":
                result = self.server.controller.preview(request)
            elif self.path == "/v1/apply":
                result = self.server.controller.apply(request)
            else:
                return self.send(404, {"error": "Unknown operation"})
            self.send(200, result)
        except (ValueError, TypeError):
            self.send(409, {"error": "Operation rejected. Check fields, current revision and confirmation. No automatic retry was made."})
        except (OSError, subprocess.SubprocessError):
            self.send(503, {"error": "Controller operation failed. Inspect process and configuration before retrying."})


def main():
    token = Path(os.environ["NATSUI_CONTROL_TOKEN_FILE"]).read_text().strip()
    if not re.fullmatch(r"[a-fA-F0-9]{64}", token):
        raise ValueError("Controller token must contain 64 hexadecimal characters")
    controller = Controller(os.environ.get("NATSUI_CONTROL_DATA", "/data"), os.environ["NATSUI_CONTROL_BASE_FILE"])
    server = Server(("0.0.0.0", 9443), Handler)
    server.daemon_threads = True
    server.controller = controller
    server.token = token
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(os.environ["NATSUI_CONTROL_CERT_FILE"], os.environ["NATSUI_CONTROL_KEY_FILE"])
    server.socket = context.wrap_socket(server.socket, server_side=True, do_handshake_on_connect=False)
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    closing = threading.Event()
    controller.start()

    def watch_child():
        while not closing.wait(1):
            with controller.lock:
                if controller.child is not None and controller.child.poll() is not None:
                    os._exit(1)

    threading.Thread(target=watch_child, daemon=True).start()
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        closing.set()
        server.server_close()
        controller.stop()


if __name__ == "__main__":
    main()
