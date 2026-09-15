"""Optional signing authority for one existing NSC account store."""

from contextlib import contextmanager
import fcntl
import hashlib
import json
import os
from pathlib import Path
import re
import secrets
import signal
import ssl
import subprocess
import tempfile
import threading
import time
from urllib.parse import urlsplit

from server import Handler, Server, encoded, permissions, read_json


class Authority:
    def __init__(self, config, binary="/usr/local/bin/nsc"):
        if set(config) - {"store", "operator", "account", "account_public_key", "resolver_url", "system_user", "ca_file", "client_cert", "client_key"}:
            raise ValueError("Unsupported account authority setting")
        for field in ("operator", "account"):
            if not re.fullmatch(r"[A-Za-z0-9_-]{1,48}", config.get(field, "")):
                raise ValueError("Invalid operator or account name")
        if not re.fullmatch(r"A[A-Z2-7]{55}", config.get("account_public_key", "")):
            raise ValueError("An existing account public key is required")
        self.config = config
        self.store = Path(config["store"]).resolve(strict=True)
        self.lockfile = open(self.store / ".natsui-authority.lock", "a")
        fcntl.flock(self.lockfile, fcntl.LOCK_EX | fcntl.LOCK_NB)
        self.binary = binary
        self.lock = threading.RLock()
        self.pending = {}
        self.published_revision = None
        self.account_dir = self.store / config["operator"] / "accounts" / config["account"]
        if self.account_dir.resolve(strict=True) != self.account_dir or not self.account_dir.is_dir():
            raise ValueError("An existing account in the exclusive NSC store is required")
        if config.get("resolver_url"):
            url = urlsplit(config["resolver_url"])
            if url.scheme != "nats" or not url.hostname or url.username or url.password or url.path not in ("", "/") or url.query or url.fragment:
                raise ValueError("Resolver publishing requires a fixed NATS URL with an explicit CA file")
            if not config.get("ca_file"):
                raise ValueError("Resolver publishing requires a TLS CA file")
            if not re.fullmatch(r"[A-Za-z0-9_-]{1,48}", config.get("system_user", "")):
                raise ValueError("An existing system user is required for resolver publishing")
        if bool(config.get("client_cert")) != bool(config.get("client_key")):
            raise ValueError("Client certificate and key must be configured together")
        self.check_store()
        self.run("env", "--operator", config["operator"], "--account", config["account"])
        self.claims()

    @contextmanager
    def operation_lock(self):
        # A queued mutation must not start after its dashboard request times out.
        if not self.lock.acquire(blocking=False):
            raise BlockingIOError("Account authority is busy")
        try:
            yield
        finally:
            self.lock.release()

    def __del__(self):
        lockfile = getattr(self, "lockfile", None)
        if lockfile:
            lockfile.close()

    def check_store(self):
        metadata = read_json(self.store / self.config["operator"] / ".nsc")
        if metadata.get("managed"):
            raise ValueError("Managed NSC stores can publish implicitly and are unsupported")

    def run(self, *args):
        # Files cap retained command output and prevent credentials reaching logs.
        with tempfile.TemporaryFile() as output, tempfile.TemporaryFile() as error:
            child = subprocess.Popen([self.binary, "-H", str(self.store), *args], stdin=subprocess.DEVNULL, stdout=output, stderr=error, cwd=self.store, env={key: value for key, value in os.environ.items() if not key.startswith(("NSC", "NATS", "NKEY"))})
            try:
                deadline = time.monotonic() + 5
                while child.poll() is None:
                    if time.monotonic() >= deadline or os.fstat(output.fileno()).st_size > 1048576 or os.fstat(error.fileno()).st_size > 1048576:
                        raise subprocess.TimeoutExpired("nsc", 5)
                    time.sleep(0.02)
                if child.returncode:
                    raise subprocess.CalledProcessError(child.returncode, "nsc")
                output.seek(0)
                result = output.read(1048577)
                if not result.strip():
                    error.seek(0)
                    result = error.read(1048577)
                if len(result) > 1048576:
                    raise ValueError("NSC output exceeds 1 MiB")
                return result.decode()
            finally:
                if child.poll() is None:
                    child.kill()
                child.wait()

    def claims(self):
        self.check_store()
        value = json.loads(self.run("describe", "account", "-n", self.config["account"], "--json"))
        if value.get("sub") != self.config["account_public_key"]:
            raise ValueError("Account identity changed")
        return value

    def revision(self):
        digest = hashlib.sha256()
        paths = sorted(self.account_dir.rglob("*.jwt"))
        if not 1 <= len(paths) <= 257:
            raise ValueError("Account store exceeds the 256-user limit")
        for path in paths:
            if path.is_symlink() or not path.resolve().is_relative_to(self.account_dir):
                raise ValueError("Account store contains a linked JWT")
            with open(path, "rb") as source:
                data = source.read(1048577)
            if len(data) > 1048576:
                raise ValueError("JWT exceeds 1 MiB")
            digest.update(str(path.relative_to(self.account_dir)).encode())
            digest.update(b"\x00")
            digest.update(data)
        return digest.hexdigest()

    def status(self):
        with self.operation_lock():
            claims = self.claims()
            raw_users = self.run("list", "users", "-o", self.config["operator"], "-a", self.config["account"], "--json").strip()
            users = json.loads(raw_users)
            if users is None:
                users = []
            current = self.revision()
            return {"identity_model": "jwt-account", "account": self.config["account"], "account_public_key": claims["sub"],
                    "revision": current, "users": users, "revocations": claims.get("nats", {}).get("revocations", {}),
                    "publishing_enabled": bool(self.config.get("resolver_url")), "resolver_accepted_this_revision": self.published_revision == current,
                    "publication_note": "Acceptance applies to the configured resolver response. Broker convergence and existing connections require separate observation."}

    def preview(self, request):
        with self.operation_lock():
            if set(request) - {"revision", "action", "name", "permissions", "expiry_hours", "owner"}:
                raise ValueError("Unsupported operation field")
            self.pending = {key: value for key, value in self.pending.items() if value["expires"] > time.monotonic()}
            if len(self.pending) >= 32:
                raise ValueError("Review limit reached")
            owner = request.get("owner", "direct")
            if not isinstance(owner, str) or not 1 <= len(owner) <= 128:
                raise ValueError("Invalid review owner")
            current = self.status()
            if request.get("revision") != current["revision"]:
                raise ValueError("Account changed. Reload before reviewing")
            action = request.get("action")
            if action not in ("create_user", "revoke_user", "export_account", "publish_account"):
                raise ValueError("Unsupported JWT account action")
            name = request.get("name", "")
            effect = "Exports the public account JWT. No resolver update is sent."
            if action in ("create_user", "revoke_user"):
                if not re.fullmatch(r"[A-Za-z0-9_-]{1,48}", name):
                    raise ValueError("Invalid user name")
                existing = next((user for user in current["users"] if user["name"] == name), None)
                if action == "create_user":
                    if existing or len(current["users"]) >= 256:
                        raise ValueError("User exists or the user limit was reached")
                    permissions(request.get("permissions"))
                    if any("," in subject for values in request["permissions"].values() for subject in values):
                        raise ValueError("Comma is unsupported in NSC permission subjects")
                    if type(request.get("expiry_hours")) is not int or not 1 <= request["expiry_hours"] <= 8760:
                        raise ValueError("An expiry from 1 to 8760 hours is required")
                    effect = "Creates a signed user credential. Existing account trust can authorize it immediately. The private credential is returned once and remains in the protected NSC store."
                else:
                    if not existing:
                        raise ValueError("User does not exist")
                    effect = "Adds a local account revocation. Access is not revoked on brokers until the account JWT is distributed."
            if action == "publish_account":
                if not self.config.get("resolver_url"):
                    raise ValueError("Resolver publishing is not configured")
                effect = "Publishes the current account JWT, including revocations, through the fixed system user. A successful response does not prove every broker has converged."
            confirmation = f"{action} {self.config['account']}" + (f"/{name}" if name else "")
            token = secrets.token_urlsafe(32)
            self.pending[token] = {"request": request.copy(), "owner": owner, "expires": time.monotonic() + 120, "confirmation": confirmation}
            return {"token": token, "confirmation": confirmation, "effect": effect, "revision": current["revision"], "expires_seconds": 120}

    def apply(self, request):
        with self.operation_lock():
            if set(request) - {"token", "confirmation", "owner"}:
                raise ValueError("Unsupported confirmation field")
            entry = self.pending.get(request.get("token", ""))
            if not entry or entry["expires"] <= time.monotonic() or entry["owner"] != request.get("owner", "direct") or entry["confirmation"] != request.get("confirmation"):
                raise ValueError("Review expired or confirmation does not match")
            self.pending.pop(request["token"])
            proposal = entry["request"]
            self.claims()
            if proposal["revision"] != self.revision():
                raise ValueError("Account changed after review")
            action = proposal["action"]
            account = self.config["account"]
            result = {"action": action, "account": account, "resolver_update_sent": False}
            if action == "create_user":
                args = ["add", "user", "-a", account, "-n", proposal["name"], "--expiry", f"{proposal['expiry_hours']}h"]
                for kind, flag in (("publish", "pub"), ("subscribe", "sub")):
                    subjects = proposal["permissions"][kind]
                    if subjects:
                        for subject in subjects:
                            # Commas split NSC list flags and therefore must not change subject meaning.
                            if "," in subject:
                                raise ValueError("Comma is unsupported in NSC permission subjects")
                            args += ["--allow-" + flag, subject]
                    else:
                        args += ["--deny-" + flag, ">"]
                self.run(*args)
                result["credentials"] = self.run("generate", "creds", "-a", account, "-n", proposal["name"], "--output-file", "--")
                result["effect"] = "Credential created. It may already be usable wherever this account is trusted."
            elif action == "revoke_user":
                self.run("revocations", "add-user", "-a", account, "-n", proposal["name"])
                result["effect"] = "Revocation saved locally. Publish or distribute the account JWT before treating access as revoked."
            elif action == "export_account":
                result["account_jwt"] = self.run("describe", "account", "-n", account, "--raw").strip()
            elif action == "publish_account":
                args = ["push", "-a", account, "--account-jwt-server-url", self.config["resolver_url"], "--system-user", self.config["system_user"], "--timeout", "2"]
                for field, flag in (("ca_file", "--ca-cert"), ("client_cert", "--client-cert"), ("client_key", "--client-key")):
                    if self.config.get(field):
                        args += [flag, self.config[field]]
                self.run(*args)
                self.published_revision = self.revision()
                result.update(resolver_update_sent=True, resolver_accepted=True)
            result["revision"] = self.revision()
            return result


def main():
    os.umask(0o077)
    token = Path(os.environ["NATSUI_CONTROL_TOKEN_FILE"]).read_text().strip()
    if not re.fullmatch(r"[a-fA-F0-9]{64}", token):
        raise ValueError("Controller token must contain 64 hexadecimal characters")
    controller = Authority(read_json(os.environ["NATSUI_JWT_CONFIG_FILE"]))
    server = Server(("0.0.0.0", 9443), Handler)
    server.daemon_threads = True
    server.controller, server.token = controller, token
    context = ssl.SSLContext(ssl.PROTOCOL_TLS_SERVER)
    context.minimum_version = ssl.TLSVersion.TLSv1_2
    context.load_cert_chain(os.environ["NATSUI_CONTROL_CERT_FILE"], os.environ["NATSUI_CONTROL_KEY_FILE"])
    server.socket = context.wrap_socket(server.socket, server_side=True, do_handshake_on_connect=False)
    signal.signal(signal.SIGTERM, lambda *_: (_ for _ in ()).throw(KeyboardInterrupt()))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()


if __name__ == "__main__":
    main()
