use axum::{
    Json,
    extract::{Request, State},
    http::{HeaderMap, Method, StatusCode, header},
    middleware::Next,
    response::{Html, IntoResponse, Redirect, Response},
};
use rusqlite::{Connection, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    io::{Read, Write},
    path::Path,
    sync::{Arc, Mutex, RwLock},
    time::{Duration, Instant},
};
use subtle::ConstantTimeEq;

const SESSION_SECONDS: u64 = 8 * 60 * 60;
const COOKIE: &str = "natsui_session";

#[derive(Clone)]
pub struct Auth(Option<Arc<Protected>>);
struct Protected {
    key: [u8; 32],
    policy: RwLock<Option<HashMap<String, Vec<String>>>>,
    sessions: Mutex<HashMap<[u8; 32], Grant>>,
    tickets: Mutex<HashMap<[u8; 32], Grant>>,
    users: Mutex<Option<Connection>>,
    identities: RwLock<HashMap<String, Identity>>,
    origin: Mutex<Option<reqwest::Url>>,
    attempts: Mutex<(Instant, u32)>,
}
#[derive(Clone)]
struct Identity {
    identity_revision: u64,
    key: [u8; 32],
    revision: u64,
    role: Role,
    enabled: bool,
}
#[derive(Clone)]
struct Grant {
    key: [u8; 32],
    expires: Instant,
    id: String,
    revision: u64,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Viewer,
    Operator,
    Admin,
}
impl Role {
    fn name(self) -> &'static str {
        match self {
            Self::Viewer => "viewer",
            Self::Operator => "operator",
            Self::Admin => "admin",
        }
    }
    fn parse(value: &str) -> Option<Self> {
        match value {
            "viewer" => Some(Self::Viewer),
            "operator" => Some(Self::Operator),
            "admin" => Some(Self::Admin),
            _ => None,
        }
    }
}
fn digest(value: &str) -> [u8; 32] {
    Sha256::digest(value.as_bytes()).into()
}
fn random_key() -> Result<String, String> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).map_err(|_| "Operating system randomness unavailable")?;
    Ok(bytes.iter().map(|b| format!("{b:02x}")).collect())
}
pub fn initialize(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let key = random_key()?;
    let mut options = std::fs::OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    writeln!(file, "{key}")?;
    file.sync_all()?;
    Ok(())
}
impl Auth {
    pub fn disabled() -> Self {
        Self(None)
    }
    pub fn configure_policy(&self, bytes: &[u8]) -> Result<(), String> {
        #[derive(Deserialize)]
        #[serde(deny_unknown_fields)]
        struct Policy {
            profiles: HashMap<String, Vec<String>>,
        }
        let protected = self
            .0
            .as_ref()
            .ok_or("Profile policy requires dashboard authentication")?;
        let policy: Policy =
            serde_json::from_slice(bytes).map_err(|_| "Invalid profile access policy")?;
        let valid = |name: &str| {
            !name.is_empty()
                && name.len() <= 48
                && name
                    .bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        };
        if bytes.len() > 65536
            || policy.profiles.len() > 8
            || policy.profiles.iter().any(|(id, users)| {
                !valid(id) || users.len() > 256 || users.iter().any(|user| !valid(user))
            })
        {
            return Err("Invalid profile access policy limits".into());
        }
        *protected
            .policy
            .write()
            .map_err(|_| "Profile policy lock unavailable")? = Some(policy.profiles);
        Ok(())
    }
    pub fn allowed_profile(&self, headers: &HeaderMap, id: &str) -> bool {
        if !self.enabled() {
            return true;
        }
        let Some((actor, _)) = self.principal(headers) else {
            return false;
        };
        if actor == "bootstrap" {
            return true;
        }
        let Some(protected) = &self.0 else {
            return false;
        };
        let Ok(policy) = protected.policy.read() else {
            return false;
        };
        policy
            .as_ref()
            .is_none_or(|profiles| profiles.get(id).is_some_and(|users| users.contains(&actor)))
    }
    pub fn enabled(&self) -> bool {
        self.0.is_some()
    }
    pub fn from_env() -> Result<Self, String> {
        match std::env::var("NATSUI_AUTH_TOKEN_FILE") {
            Err(std::env::VarError::NotPresent) => Ok(Self::disabled()),
            Err(_) => Err("NATSUI_AUTH_TOKEN_FILE must be a valid path".into()),
            Ok(path) => Self::from_file(Path::new(&path)),
        }
    }
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let mut value = String::new();
        std::fs::File::open(path)
            .and_then(|f| f.take(513).read_to_string(&mut value))
            .map_err(|_| "Dashboard access key file could not be read")?;
        if value.len() > 512 {
            return Err("Invalid dashboard access key file".into());
        }
        Self::from_key(value.trim())
    }
    pub(crate) fn from_key(value: &str) -> Result<Self, String> {
        if value.len() != 64 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(
                "Dashboard access key must be 64 hexadecimal characters generated by --init-auth"
                    .into(),
            );
        }
        Ok(Self(Some(Arc::new(Protected {
            key: digest(value),
            policy: RwLock::new(None),
            sessions: Mutex::new(HashMap::new()),
            tickets: Mutex::new(HashMap::new()),
            users: Mutex::new(None),
            identities: RwLock::new(HashMap::new()),
            origin: Mutex::new(None),
            attempts: Mutex::new((Instant::now(), 0)),
        }))))
    }
    pub fn open_users(&self, directory: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let Some(protected) = &self.0 else {
            return Ok(());
        };
        let db = Connection::open(directory.join("access.sqlite3"))?;
        db.busy_timeout(Duration::from_secs(2))?;
        db.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE IF NOT EXISTS dashboard_users (id TEXT PRIMARY KEY, role TEXT NOT NULL CHECK(role IN ('viewer','operator','admin')), key_digest BLOB UNIQUE NOT NULL, enabled INTEGER NOT NULL, revision INTEGER NOT NULL);")?;
        db.execute_batch("CREATE TABLE IF NOT EXISTS dashboard_revision (id INTEGER PRIMARY KEY CHECK(id=1), value INTEGER NOT NULL); INSERT OR IGNORE INTO dashboard_revision SELECT 1, COALESCE(MAX(revision),0) FROM dashboard_users;")?;
        let has_identity: bool=db.query_row("SELECT EXISTS(SELECT 1 FROM pragma_table_info('dashboard_users') WHERE name='identity_revision')",[],|row|row.get(0))?;
        if !has_identity {
            db.execute_batch("BEGIN; ALTER TABLE dashboard_users ADD COLUMN identity_revision INTEGER NOT NULL DEFAULT 0; UPDATE dashboard_users SET identity_revision=revision; COMMIT;")?;
        }
        let mut identities = HashMap::new();
        {
            let mut query = db.prepare(
                "SELECT id,role,key_digest,enabled,revision,identity_revision FROM dashboard_users LIMIT 257",
            )?;
            let rows = query.query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Vec<u8>>(2)?,
                    row.get::<_, bool>(3)?,
                    row.get::<_, u64>(4)?,
                    row.get::<_, u64>(5)?,
                ))
            })?;
            for row in rows {
                let (id, role, key, enabled, revision, identity_revision) = row?;
                identities.insert(
                    id,
                    Identity {
                        identity_revision,
                        key: key.try_into().map_err(|_| "Invalid dashboard key digest")?,
                        role: Role::parse(&role).ok_or("Invalid stored dashboard role")?,
                        enabled,
                        revision,
                    },
                );
            }
        }
        if identities.len() > 256 {
            return Err("Dashboard user limit exceeded".into());
        }
        *protected
            .identities
            .write()
            .map_err(|_| "Identity cache unavailable")? = identities;
        *protected
            .users
            .lock()
            .map_err(|_| "User storage lock unavailable")? = Some(db);
        Ok(())
    }
    pub fn configure_origin(&self, origin: &str) -> Result<(), String> {
        let protected = self
            .0
            .as_ref()
            .ok_or("Public HTTPS deployment requires dashboard authentication")?;
        let url = reqwest::Url::parse(origin).map_err(|_| "Invalid NATSUI_PUBLIC_URL")?;
        if url.scheme() != "https"
            || url.host_str().is_none()
            || url.path() != "/"
            || url.query().is_some()
            || url.fragment().is_some()
            || !url.username().is_empty()
            || url.password().is_some()
        {
            return Err(
                "NATSUI_PUBLIC_URL must be an HTTPS origin without credentials or a path".into(),
            );
        }
        *protected
            .origin
            .lock()
            .map_err(|_| "Origin lock unavailable")? = Some(url);
        Ok(())
    }
    pub fn origin(&self) -> Option<reqwest::Url> {
        self.0.as_ref()?.origin.lock().ok()?.clone()
    }
    fn authenticate(&self, key: &str) -> Result<Grant, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let now = Instant::now();
        {
            let mut attempts = protected
                .attempts
                .lock()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
            if now.duration_since(attempts.0) >= Duration::from_secs(60) {
                *attempts = (now, 0);
            }
            if attempts.1 >= 30 {
                return Err(StatusCode::TOO_MANY_REQUESTS);
            }
            attempts.1 += 1;
        }
        if key.len() != 64 {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let hashed = digest(key);
        if bool::from(protected.key.ct_eq(&hashed)) {
            return Ok(Grant {
                expires: now,
                id: "bootstrap".into(),
                key: hashed,
                revision: 0,
            });
        }
        let identities = protected
            .identities
            .read()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        identities
            .iter()
            .find(|(_, identity)| identity.enabled && bool::from(identity.key.ct_eq(&hashed)))
            .map(|(id, identity)| Grant {
                expires: now,
                id: id.clone(),
                key: hashed,
                revision: identity.revision,
            })
            .ok_or(StatusCode::UNAUTHORIZED)
    }
    pub(crate) fn external_session(
        &self,
        user: &str,
        identity_revision: u64,
        headers: &HeaderMap,
    ) -> Result<String, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::FORBIDDEN)?;
        let identity = protected
            .identities
            .read()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .get(user)
            .filter(|identity| identity.enabled && identity.identity_revision == identity_revision)
            .cloned()
            .ok_or(StatusCode::FORBIDDEN)?;
        let token = self.new_session(
            Grant {
                key: identity.key,
                id: user.to_owned(),
                revision: identity.revision,
                expires: Instant::now(),
            },
            session(headers),
        )?;
        Ok(app_cookie(self, &token, SESSION_SECONDS))
    }
    fn issue(&self, key: &str, old: Option<&str>) -> Result<String, StatusCode> {
        let identity = self.authenticate(key)?;
        self.new_session(identity, old)
    }
    fn ticket(&self, key: &str) -> Result<String, StatusCode> {
        let mut identity = self.authenticate(key)?;
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let now = Instant::now();
        let token = random_key().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut tickets = protected
            .tickets
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        tickets.retain(|_, grant| grant.expires > now);
        if tickets.len() >= 8 {
            return Err(StatusCode::TOO_MANY_REQUESTS);
        }
        identity.expires = now + Duration::from_secs(60);
        tickets.insert(digest(&token), identity);
        Ok(token)
    }
    fn exchange(&self, token: &str, old: Option<&str>) -> Result<String, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        if token.len() != 64 {
            return Err(StatusCode::UNAUTHORIZED);
        }
        let identity = protected
            .tickets
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .remove(&digest(token));
        let identity = identity
            .filter(|identity| identity.expires > Instant::now())
            .ok_or(StatusCode::UNAUTHORIZED)?;
        if self.identity_role(&identity).is_none() {
            return Err(StatusCode::UNAUTHORIZED);
        }
        self.new_session(identity, old)
    }
    fn new_session(&self, mut identity: Grant, old: Option<&str>) -> Result<String, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let now = Instant::now();
        let token = random_key().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut sessions = protected
            .sessions
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        sessions.retain(|_, grant| grant.expires > now);
        if let Some(old) = old {
            sessions.remove(&digest(old));
        }
        if sessions.len() >= 32
            && let Some(oldest) = sessions
                .iter()
                .min_by_key(|(_, grant)| grant.expires)
                .map(|(key, _)| *key)
        {
            sessions.remove(&oldest);
        }
        identity.expires = now + Duration::from_secs(SESSION_SECONDS);
        sessions.insert(digest(&token), identity);
        Ok(token)
    }
    fn valid(&self, token: Option<&str>) -> bool {
        let Some(protected) = &self.0 else {
            return true;
        };
        let Some(token) = token.filter(|t| t.len() == 64) else {
            return false;
        };
        let Ok(mut sessions) = protected.sessions.lock() else {
            return false;
        };
        sessions.retain(|_, grant| grant.expires > Instant::now());
        sessions
            .get(&digest(token))
            .is_some_and(|identity| self.identity_role(identity).is_some())
    }
    fn identity_role(&self, grant: &Grant) -> Option<Role> {
        let protected = self.0.as_ref()?;
        if grant.id == "bootstrap" {
            return bool::from(protected.key.ct_eq(&grant.key)).then_some(Role::Admin);
        }
        let identities = protected.identities.read().ok()?;
        let identity = identities.get(&grant.id)?;
        (identity.enabled
            && identity.revision == grant.revision
            && bool::from(identity.key.ct_eq(&grant.key)))
        .then_some(identity.role)
    }
    fn principal(&self, headers: &HeaderMap) -> Option<(String, Role)> {
        let Some(protected) = &self.0 else {
            return Some(("trusted-local".into(), Role::Admin));
        };
        let token = session(headers)?;
        let sessions = protected.sessions.lock().ok()?;
        let grant = sessions
            .get(&digest(token))
            .filter(|g| g.expires > Instant::now())?;
        self.identity_role(grant)
            .map(|role| (grant.id.clone(), role))
    }
    pub fn role(&self, headers: &HeaderMap) -> Option<Role> {
        self.principal(headers).map(|(_, role)| role)
    }
    pub fn review_owner(&self, headers: &HeaderMap) -> String {
        if !self.enabled() {
            return "trusted-local".into();
        }
        session(headers)
            .map(|token| {
                digest(token)
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect()
            })
            .unwrap_or_default()
    }
    pub fn actor(&self, headers: &HeaderMap) -> String {
        self.principal(headers)
            .map(|(id, _)| id)
            .unwrap_or_else(|| "unavailable".into())
    }
    fn revoke(&self, token: Option<&str>) -> Result<(), StatusCode> {
        if let (Some(protected), Some(token)) = (&self.0, token) {
            protected
                .sessions
                .lock()
                .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
                .remove(&digest(token));
        }
        Ok(())
    }
}
fn session(headers: &HeaderMap) -> Option<&str> {
    headers
        .get(header::COOKIE)?
        .to_str()
        .ok()?
        .split(';')
        .filter_map(|part| part.trim().split_once('='))
        .find_map(|(name, value)| (name == COOKIE).then_some(value))
}
fn cookie(token: &str, age: u64) -> String {
    // HTTP is restricted to loopback. Public HTTPS/proxy deployment is a separate mode.
    format!("{COOKIE}={token}; Path=/; HttpOnly; SameSite=Strict; Max-Age={age}")
}
fn app_cookie(auth: &Auth, token: &str, age: u64) -> String {
    let mut value = cookie(token, age);
    if auth.origin().is_some() {
        value.push_str("; Secure");
    }
    value
}
pub async fn guard(State(auth): State<Auth>, request: Request, next: Next) -> Response {
    if !auth.enabled() {
        return next.run(request).await;
    }
    if request.method() != Method::GET
        && request.method() != Method::HEAD
        && request
            .headers()
            .get("x-natsui-request")
            .and_then(|h| h.to_str().ok())
            != Some("1")
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let path = request.uri().path();
    let public = matches!(
        path,
        "/login"
            | "/login.js"
            | "/style.css"
            | "/fibril.css"
            | "/kitten.svg"
            | "/oidc-complete.js"
            | "/api/auth/oidc"
            | "/api/auth/oidc/start"
            | "/api/auth/oidc/callback"
            | "/api/auth/login"
            | "/api/auth/ticket"
            | "/api/auth/exchange"
            | "/healthz"
            | "/readyz"
    );
    let role = if public {
        None
    } else {
        auth.role(request.headers())
    };
    if !public && role.is_none() {
        return if path == "/" {
            Redirect::to("/login").into_response()
        } else {
            StatusCode::UNAUTHORIZED.into_response()
        };
    }
    if !public {
        let admin = path.starts_with("/api/users")
            || (path.starts_with("/api/profiles")
                && request.method() != Method::GET
                && path != "/api/profiles/select")
            || path.starts_with("/api/managed")
            || path.starts_with("/api/nats-users")
            || (path == "/api/settings" && request.method() != Method::GET);
        let mutation = request.method() != Method::GET
            && request.method() != Method::HEAD
            && path != "/api/auth/logout"
            && path != "/api/profiles/select";
        if (admin && role != Some(Role::Admin))
            || (mutation && !admin && role == Some(Role::Viewer))
        {
            return StatusCode::FORBIDDEN.into_response();
        }
    }
    next.run(request).await
}
pub async fn page(State(app): State<crate::App>, headers: HeaderMap) -> Response {
    if !app.auth.enabled() || app.auth.valid(session(&headers)) {
        return Redirect::to("/").into_response();
    }
    Html(include_str!("../web/login.html")).into_response()
}
#[derive(Deserialize)]
pub struct Credentials {
    key: String,
}
pub async fn login(
    State(app): State<crate::App>,
    headers: HeaderMap,
    Json(credentials): Json<Credentials>,
) -> Response {
    match app.auth.issue(&credentials.key, session(&headers)) {
        Ok(token) => (
            [(
                header::SET_COOKIE,
                app_cookie(&app.auth, &token, SESSION_SECONDS),
            )],
            StatusCode::NO_CONTENT,
        )
            .into_response(),
        Err(status) => {
            let mut response = status.into_response();
            if status == StatusCode::TOO_MANY_REQUESTS {
                response
                    .headers_mut()
                    .insert(header::RETRY_AFTER, "60".parse().unwrap());
            }
            response
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Ticket {
    token: String,
}
pub async fn ticket(
    State(app): State<crate::App>,
    Json(credentials): Json<Credentials>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    app.auth
        .ticket(&credentials.key)
        .map(|token| Json(serde_json::json!({"token":token,"expires_in":60})))
}
pub async fn exchange(
    State(app): State<crate::App>,
    headers: HeaderMap,
    Json(ticket): Json<Ticket>,
) -> Response {
    match app.auth.exchange(&ticket.token, session(&headers)) {
        Ok(token) => (
            [(
                header::SET_COOKIE,
                app_cookie(&app.auth, &token, SESSION_SECONDS),
            )],
            StatusCode::NO_CONTENT,
        )
            .into_response(),
        Err(status) => status.into_response(),
    }
}
pub fn initialize_if_missing(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    match initialize(path) {
        Ok(()) => Ok(()),
        Err(error)
            if error
                .downcast_ref::<std::io::Error>()
                .is_some_and(|e| e.kind() == std::io::ErrorKind::AlreadyExists) =>
        {
            Auth::from_file(path)?;
            Ok(())
        }
        Err(error) => Err(error),
    }
}
pub async fn login_link(port: u16) -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::var("NATSUI_AUTH_TOKEN_FILE")
        .map_err(|_| "NATSUI_AUTH_TOKEN_FILE is required for login")?;
    Auth::from_file(Path::new(&path))?;
    let mut key = String::new();
    std::fs::File::open(path)?
        .take(513)
        .read_to_string(&mut key)?;
    let address = format!("http://127.0.0.1:{port}");
    let client = reqwest::Client::builder()
        .no_proxy()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(5))
        .build()?;
    let response = client
        .post(format!("{address}/api/auth/ticket"))
        .header("x-natsui-request", "1")
        .json(&serde_json::json!({"key":key.trim()}))
        .send()
        .await?;
    if !response.status().is_success() {
        return Err(format!("Login link unavailable (HTTP {})", response.status()).into());
    }
    let value: serde_json::Value = response.json().await?;
    let token = value["token"]
        .as_str()
        .filter(|t| t.len() == 64 && t.bytes().all(|b| b.is_ascii_hexdigit()))
        .ok_or("Invalid login response")?;
    println!("Open this one-time link within 60 seconds:");
    let public = std::env::var("NATSUI_PUBLIC_URL").unwrap_or(address);
    println!("{}/login#ticket={token}", public.trim_end_matches('/'));
    Ok(())
}
pub async fn logout(State(app): State<crate::App>, headers: HeaderMap) -> Response {
    match app.auth.revoke(session(&headers)) {
        Ok(()) => (
            [(header::SET_COOKIE, app_cookie(&app.auth, "", 0))],
            StatusCode::NO_CONTENT,
        )
            .into_response(),
        Err(status) => status.into_response(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{Router, body::Body, routing::get};
    use tower::ServiceExt;
    const KEY: &str = "abababababababababababababababababababababababababababababababab";
    #[test]
    fn tickets_are_single_use_expiring_bounded_and_require_key() {
        let auth = Auth::from_key(KEY).unwrap();
        assert_eq!(auth.ticket("wrong"), Err(StatusCode::UNAUTHORIZED));
        let ticket = auth.ticket(KEY).unwrap();
        assert!(!auth.valid(Some(&ticket)));
        let session = auth.exchange(&ticket, None).unwrap();
        assert!(auth.valid(Some(&session)));
        assert_eq!(auth.exchange(&ticket, None), Err(StatusCode::UNAUTHORIZED));
        let expired = auth.ticket(KEY).unwrap();
        auth.0
            .as_ref()
            .unwrap()
            .tickets
            .lock()
            .unwrap()
            .get_mut(&digest(&expired))
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert_eq!(auth.exchange(&expired, None), Err(StatusCode::UNAUTHORIZED));
        for _ in 0..8 {
            auth.ticket(KEY).unwrap();
        }
        assert_eq!(auth.ticket(KEY), Err(StatusCode::TOO_MANY_REQUESTS));
        assert_eq!(
            Auth::from_key(KEY).unwrap().exchange(&ticket, None),
            Err(StatusCode::UNAUTHORIZED)
        );
    }
    #[test]
    fn idempotent_initialization_preserves_key_and_rejects_invalid_files() {
        let path = std::env::temp_dir().join(format!("natsui-init-{}", random_key().unwrap()));
        initialize_if_missing(&path).unwrap();
        let original = std::fs::read(&path).unwrap();
        initialize_if_missing(&path).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), original);
        std::fs::write(&path, "invalid").unwrap();
        assert!(initialize_if_missing(&path).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "invalid");
        std::fs::remove_file(path).unwrap();
    }
    #[test]
    fn session_expiry_revocation_rotation_and_limits() {
        let auth = Auth::from_key(KEY).unwrap();
        assert!(!auth.valid(None));
        assert_eq!(
            auth.issue(&"0".repeat(64), None),
            Err(StatusCode::UNAUTHORIZED)
        );
        let first = auth.issue(KEY, None).unwrap();
        assert!(auth.valid(Some(&first)));
        let second = auth.issue(KEY, Some(&first)).unwrap();
        assert!(!auth.valid(Some(&first)));
        assert!(auth.valid(Some(&second)));
        auth.revoke(Some(&second)).unwrap();
        assert!(!auth.valid(Some(&second)));
        let third = auth.issue(KEY, None).unwrap();
        auth.0
            .as_ref()
            .unwrap()
            .sessions
            .lock()
            .unwrap()
            .get_mut(&digest(&third))
            .unwrap()
            .expires = Instant::now() - Duration::from_secs(1);
        assert!(!auth.valid(Some(&third)));
        assert!(!Auth::from_key(KEY).unwrap().valid(Some(&first)));
        for _ in 0..30 {
            let _ = auth.issue("wrong", None);
        }
        assert_eq!(auth.issue(KEY, None), Err(StatusCode::TOO_MANY_REQUESTS));
        let c = cookie(&first, SESSION_SECONDS);
        for flag in ["HttpOnly", "SameSite=Strict", "Path=/", "Max-Age=28800"] {
            assert!(c.contains(flag));
        }
    }
    #[test]
    fn sessions_are_bounded_and_rate_limit_recovers() {
        let auth = Auth::from_key(KEY).unwrap();
        let first = auth.issue(KEY, None).unwrap();
        for _ in 0..40 {
            *auth.0.as_ref().unwrap().attempts.lock().unwrap() =
                (Instant::now() - Duration::from_secs(61), 30);
            auth.issue(KEY, None).unwrap();
        }
        assert_eq!(auth.0.as_ref().unwrap().sessions.lock().unwrap().len(), 32);
        assert!(!auth.valid(Some(&first)));
    }
    #[test]
    fn key_files_fail_closed_and_initialization_never_overwrites() {
        let path = std::env::temp_dir().join(format!("natsui-auth-{}", random_key().unwrap()));
        assert!(Auth::from_file(&path).is_err());
        initialize(&path).unwrap();
        assert!(Auth::from_file(&path).unwrap().enabled());
        assert!(initialize(&path).is_err());
        for value in ["", "password", &"0".repeat(513)] {
            std::fs::write(&path, value).unwrap();
            assert!(Auth::from_file(&path).is_err());
        }
        std::fs::remove_file(path).unwrap();
    }
    #[tokio::test]
    async fn guard_protects_reads_writes_assets_and_unknown_routes() {
        let auth = Auth::from_key(KEY).unwrap();
        let token = auth.issue(KEY, None).unwrap();
        let app = Router::new()
            .route("/", get(|| async { "private" }))
            .fallback(|| async { "private" })
            .layer(axum::middleware::from_fn_with_state(auth, guard))
            .layer(axum::middleware::from_fn(crate::local_request));
        for (path, method, credential, origin, request_header, expected) in [
            (
                "/",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::SEE_OTHER,
            ),
            (
                "/api/snapshot",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/messages/ORDERS/1",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/app.js",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/unknown",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/config/apply",
                "POST",
                false,
                "http://localhost:4321",
                true,
                StatusCode::UNAUTHORIZED,
            ),
            (
                "/api/auth/login",
                "POST",
                false,
                "http://localhost:4321",
                false,
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/auth/login",
                "POST",
                false,
                "http://evil.example",
                true,
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/settings",
                "PUT",
                true,
                "http://localhost:4321",
                false,
                StatusCode::FORBIDDEN,
            ),
            (
                "/api/settings",
                "PUT",
                true,
                "http://localhost:4321",
                true,
                StatusCode::OK,
            ),
            (
                "/api/snapshot",
                "GET",
                true,
                "http://localhost:4321",
                false,
                StatusCode::OK,
            ),
            (
                "/login",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::OK,
            ),
            (
                "/healthz",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::OK,
            ),
            (
                "/readyz",
                "GET",
                false,
                "http://localhost:4321",
                false,
                StatusCode::OK,
            ),
        ] {
            let mut req = axum::http::Request::builder()
                .uri(path)
                .method(method)
                .header("host", "localhost:4321")
                .header("origin", origin);
            if credential {
                req = req.header("cookie", format!("{COOKIE}={token}"));
            }
            if request_header {
                req = req.header("x-natsui-request", "1");
            }
            let response = app
                .clone()
                .oneshot(req.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{method} {path}");
        }
    }
    #[tokio::test]
    async fn real_login_logout_cookie_and_body_limit() {
        use tokio::sync::RwLock;
        let dir = std::env::temp_dir().join(format!("natsui-login-{}", random_key().unwrap()));
        let auth = Auth::from_key(KEY).unwrap();
        let app = crate::App {
            auth: auth.clone(),
            editor: crate::editing::Editor::new(false),
            db: crate::store::Database::open(dir.to_str().unwrap()).unwrap(),
            current: Arc::new(RwLock::new(crate::simulation::snapshot("auth-test", 0))),
            nats: Arc::new(RwLock::new(None)),
            monitor: crate::monitoring::Monitor::new("").unwrap(),
            demo: true,
            prefix: "$JS.API".into(),
            scope: "auth-test".into(),
            connection: None,
            settings_cache: Arc::new(RwLock::new(crate::store::Settings::default())),
        };
        let router = crate::router(app);
        let request = |path: &str, method: &str, body: String, cookie: Option<&str>| {
            let mut builder = axum::http::Request::builder()
                .uri(path)
                .method(method)
                .header("host", "localhost:4321")
                .header("origin", "http://localhost:4321")
                .header("x-natsui-request", "1")
                .header("content-type", "application/json");
            if let Some(c) = cookie {
                builder = builder.header("cookie", c);
            }
            builder.body(Body::from(body)).unwrap()
        };
        let oversized = router
            .clone()
            .oneshot(request(
                "/api/auth/login",
                "POST",
                serde_json::json!({"key":"a".repeat(2000)}).to_string(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(oversized.status(), StatusCode::PAYLOAD_TOO_LARGE);
        let invalid = router
            .clone()
            .oneshot(request(
                "/api/auth/login",
                "POST",
                serde_json::json!({"key":"wrong"}).to_string(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(invalid.status(), StatusCode::UNAUTHORIZED);
        let login = router
            .clone()
            .oneshot(request(
                "/api/auth/login",
                "POST",
                serde_json::json!({"key":KEY}).to_string(),
                None,
            ))
            .await
            .unwrap();
        assert_eq!(login.status(), StatusCode::NO_CONTENT);
        assert!(login.headers().contains_key("content-security-policy"));
        assert_eq!(login.headers()[header::CACHE_CONTROL], "no-store");
        let cookie = login.headers()[header::SET_COOKIE]
            .to_str()
            .unwrap()
            .split(';')
            .next()
            .unwrap()
            .to_owned();
        assert!(!cookie.contains(KEY));
        let snapshot = router
            .clone()
            .oneshot(request(
                "/api/snapshot",
                "GET",
                String::new(),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(snapshot.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(snapshot.into_body(), 1_000_000)
            .await
            .unwrap();
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&bytes).unwrap()["dashboard"]["auth_enabled"],
            true
        );
        let logout = router
            .clone()
            .oneshot(request(
                "/api/auth/logout",
                "POST",
                String::new(),
                Some(&cookie),
            ))
            .await
            .unwrap();
        assert_eq!(logout.status(), StatusCode::NO_CONTENT);
        assert!(
            logout.headers()[header::SET_COOKIE]
                .to_str()
                .unwrap()
                .contains("Max-Age=0")
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request(
                    "/api/snapshot",
                    "GET",
                    String::new(),
                    Some(&cookie)
                ))
                .await
                .unwrap()
                .status(),
            StatusCode::UNAUTHORIZED
        );
        assert_eq!(
            router
                .clone()
                .oneshot(request("/healthz", "GET", String::new(), None))
                .await
                .unwrap()
                .status(),
            StatusCode::OK
        );
        drop(router);
        for entry in std::fs::read_dir(&dir).unwrap() {
            std::fs::remove_file(entry.unwrap().path()).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NewUser {
    id: String,
    role: Role,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserChange {
    revision: u64,
    action: String,
    role: Option<Role>,
}
impl Auth {
    fn users(&self) -> Result<serde_json::Value, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let identities = protected
            .identities
            .read()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut rows: Vec<_> = identities.iter().map(|(id, identity)| serde_json::json!({"id":id,"role":identity.role,"enabled":identity.enabled,"revision":identity.revision,"identity_revision":identity.identity_revision})).collect();
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        Ok(
            serde_json::json!({"users":rows,"bootstrap":"The configured dashboard key retains recovery administrator access."}),
        )
    }
    fn add_user(&self, user: NewUser) -> Result<serde_json::Value, StatusCode> {
        if user.id.is_empty()
            || user.id.len() > 48
            || user.id == "bootstrap"
            || !user
                .id
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
        {
            return Err(StatusCode::BAD_REQUEST);
        }
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let mut users = protected
            .users
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let db = users.as_mut().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let tx = db
            .transaction()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let count: u64 = tx
            .query_row("SELECT count(*) FROM dashboard_users", [], |r| r.get(0))
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if count >= 256 {
            return Err(StatusCode::CONFLICT);
        }
        let revision = next_user_revision(&tx)?;
        let key = random_key().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        tx.execute("INSERT INTO dashboard_users(id,role,key_digest,enabled,revision,identity_revision) VALUES(?1,?2,?3,1,?4,?4)", params![user.id,user.role.name(),digest(&key).as_slice(),revision]).map_err(|_| StatusCode::CONFLICT)?;
        tx.commit().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        protected
            .identities
            .write()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?
            .insert(
                user.id.clone(),
                Identity {
                    identity_revision: revision,
                    key: digest(&key),
                    role: user.role,
                    enabled: true,
                    revision,
                },
            );
        Ok(
            serde_json::json!({"id":user.id,"role":user.role,"key":key,"revision":revision,"identity_revision":revision}),
        )
    }
    fn change_user(&self, id: &str, change: UserChange) -> Result<serde_json::Value, StatusCode> {
        let protected = self.0.as_ref().ok_or(StatusCode::NOT_FOUND)?;
        let mut users = protected
            .users
            .lock()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let db = users.as_mut().ok_or(StatusCode::SERVICE_UNAVAILABLE)?;
        let tx = db
            .transaction()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let revision = next_user_revision(&tx)?;
        let mut key = None;
        let count = match change.action.as_str() {
            "rotate" if change.role.is_none() => {
                let token = random_key().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
                let count = tx.execute("UPDATE dashboard_users SET key_digest=?1,revision=?4 WHERE id=?2 AND revision=?3", params![digest(&token).as_slice(),id,change.revision,revision]);
                key = Some(token);
                count
            }
            "role" if change.role.is_some() => tx.execute("UPDATE dashboard_users SET role=?1,revision=?4 WHERE id=?2 AND revision=?3", params![change.role.unwrap().name(),id,change.revision,revision]),
            "disable" | "enable" if change.role.is_none() => tx.execute("UPDATE dashboard_users SET enabled=?1,revision=?4 WHERE id=?2 AND revision=?3", params![change.action=="enable",id,change.revision,revision]),
            "delete" if change.role.is_none() => tx.execute("DELETE FROM dashboard_users WHERE id=?1 AND revision=?2",params![id,change.revision]),
            _ => return Err(StatusCode::BAD_REQUEST),
        }.map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if count != 1 {
            return Err(StatusCode::CONFLICT);
        }
        tx.commit().map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        let mut identities = protected
            .identities
            .write()
            .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
        if change.action == "delete" {
            identities.remove(id);
        } else if let Some(identity) = identities.get_mut(id) {
            identity.revision = revision;
            if let Some(key) = &key {
                identity.key = digest(key);
            }
            if let Some(role) = change.role {
                identity.role = role;
            }
            if change.action == "enable" || change.action == "disable" {
                identity.enabled = change.action == "enable";
            }
        }
        Ok(serde_json::json!({"id":id,"key":key,"revision":revision}))
    }
}
// Revisions remain unique after deletion, recreation and restart. The counter
// shares the user mutation transaction, so failed changes cannot publish a revision.
fn next_user_revision(tx: &rusqlite::Transaction<'_>) -> Result<u64, StatusCode> {
    tx.query_row("UPDATE dashboard_revision SET value=value+1 WHERE id=1 AND value<9007199254740991 RETURNING value", [], |row| row.get(0)).map_err(|_| StatusCode::SERVICE_UNAVAILABLE)
}
pub async fn me(State(app): State<crate::App>, headers: HeaderMap) -> Json<serde_json::Value> {
    Json(serde_json::json!({"enabled":app.auth.enabled(),"role":app.auth.role(&headers)}))
}
pub async fn users(State(app): State<crate::App>) -> Result<Json<serde_json::Value>, StatusCode> {
    app.auth.users().map(Json)
}
pub async fn create_user(
    State(app): State<crate::App>,
    headers: HeaderMap,
    Json(user): Json<NewUser>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let detail = format!(
        "actor={}, action=create, user={}",
        app.auth.actor(&headers),
        user.id
    );
    app.db
        .event(&app.scope, app.demo, "dashboard_user_requested", &detail)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let auth = app.auth.clone();
    let mut result = tokio::task::spawn_blocking(move || auth.add_user(user))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)??;
    result["audit_saved"] = app
        .db
        .event(&app.scope, app.demo, "dashboard_user_changed", &detail)
        .await
        .is_ok()
        .into();
    Ok(Json(result))
}
pub async fn change_user(
    State(app): State<crate::App>,
    axum::extract::Path(id): axum::extract::Path<String>,
    headers: HeaderMap,
    Json(change): Json<UserChange>,
) -> Result<Json<serde_json::Value>, StatusCode> {
    let detail = format!(
        "actor={}, action={}, user={id}",
        app.auth.actor(&headers),
        change.action
    );
    app.db
        .event(&app.scope, app.demo, "dashboard_user_requested", &detail)
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)?;
    let auth = app.auth.clone();
    let mut result = tokio::task::spawn_blocking(move || auth.change_user(&id, change))
        .await
        .map_err(|_| StatusCode::SERVICE_UNAVAILABLE)??;
    result["audit_saved"] = app
        .db
        .event(&app.scope, app.demo, "dashboard_user_changed", &detail)
        .await
        .is_ok()
        .into();
    Ok(Json(result))
}

#[cfg(test)]
mod user_tests {
    #[test]
    fn recreated_username_does_not_revive_sessions_or_tickets() {
        let directory =
            std::env::temp_dir().join(format!("natsui-user-incarnation-{}", random_key().unwrap()));
        std::fs::create_dir(&directory).unwrap();
        let auth = Auth::from_key(KEY).unwrap();
        auth.open_users(&directory).unwrap();
        let user = auth
            .add_user(NewUser {
                id: "alice".into(),
                role: Role::Admin,
            })
            .unwrap();
        let key = user["key"].as_str().unwrap();
        let session = auth.issue(key, None).unwrap();
        let ticket = auth.ticket(key).unwrap();
        auth.change_user(
            "alice",
            UserChange {
                revision: 1,
                action: "delete".into(),
                role: None,
            },
        )
        .unwrap();
        let replacement = auth
            .add_user(NewUser {
                id: "alice".into(),
                role: Role::Viewer,
            })
            .unwrap();
        assert!(replacement["revision"].as_u64().unwrap() > user["revision"].as_u64().unwrap());
        assert_eq!(
            auth.change_user(
                "alice",
                UserChange {
                    revision: user["revision"].as_u64().unwrap(),
                    action: "delete".into(),
                    role: None
                }
            ),
            Err(StatusCode::CONFLICT)
        );
        assert!(!auth.valid(Some(&session)));
        assert_eq!(
            auth.exchange(&ticket, None).unwrap_err(),
            StatusCode::UNAUTHORIZED
        );
        assert!(
            auth.issue(replacement["key"].as_str().unwrap(), None)
                .is_ok()
        );
        drop(auth);
        std::fs::remove_dir_all(directory).unwrap();
    }

    use super::*;
    use axum::{Router, body::Body, routing::get};
    use tower::ServiceExt;
    const KEY: &str = "abababababababababababababababababababababababababababababababab";
    #[tokio::test]
    async fn users_persist_and_roles_rotation_disable_and_revisions_are_enforced() {
        let dir = std::env::temp_dir().join(format!("natsui-users-{}", random_key().unwrap()));
        std::fs::create_dir(&dir).unwrap();
        let auth = Auth::from_key(KEY).unwrap();
        auth.open_users(&dir).unwrap();
        let mut keys = Vec::new();
        for (id, role) in [
            ("viewer", Role::Viewer),
            ("operator", Role::Operator),
            ("admin", Role::Admin),
        ] {
            let result = auth
                .add_user(NewUser {
                    id: id.into(),
                    role,
                })
                .unwrap();
            keys.push(result["key"].as_str().unwrap().to_owned());
        }
        let public = auth.users().unwrap().to_string();
        for key in &keys {
            assert!(!public.contains(key));
        }
        let router = Router::new()
            .fallback(
                get(|| async { "ok" })
                    .post(|| async { "ok" })
                    .put(|| async { "ok" }),
            )
            .layer(axum::middleware::from_fn_with_state(auth.clone(), guard));
        for (index, key) in keys.iter().enumerate() {
            let token = auth.issue(key, None).unwrap();
            for (path, method, expected) in [
                ("/api/snapshot", "GET", StatusCode::OK),
                (
                    "/api/users",
                    "GET",
                    if index == 2 {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                ),
                (
                    "/api/config/apply",
                    "POST",
                    if index > 0 {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                ),
                (
                    "/api/settings",
                    "PUT",
                    if index == 2 {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                ),
                (
                    "/api/profiles",
                    "POST",
                    if index == 2 {
                        StatusCode::OK
                    } else {
                        StatusCode::FORBIDDEN
                    },
                ),
            ] {
                let request = axum::http::Request::builder()
                    .uri(path)
                    .method(method)
                    .header("cookie", format!("{COOKIE}={token}"))
                    .header("x-natsui-request", "1")
                    .body(Body::empty())
                    .unwrap();
                assert_eq!(
                    router.clone().oneshot(request).await.unwrap().status(),
                    expected,
                    "{index}: {path}"
                );
            }
        }
        let old = auth.issue(&keys[0], None).unwrap();
        let ticket = auth.ticket(&keys[0]).unwrap();
        let rotated = auth
            .change_user(
                "viewer",
                UserChange {
                    revision: 1,
                    action: "rotate".into(),
                    role: None,
                },
            )
            .unwrap();
        assert!(!auth.valid(Some(&old)));
        assert!(auth.exchange(&ticket, None).is_err());
        assert!(auth.issue(&keys[0], None).is_err());
        let fresh = rotated["key"].as_str().unwrap();
        assert!(auth.issue(fresh, None).is_ok());
        assert_eq!(
            auth.change_user(
                "viewer",
                UserChange {
                    revision: 1,
                    action: "disable".into(),
                    role: None
                }
            ),
            Err(StatusCode::CONFLICT)
        );
        auth.change_user(
            "viewer",
            UserChange {
                revision: rotated["revision"].as_u64().unwrap(),
                action: "disable".into(),
                role: None,
            },
        )
        .unwrap();
        assert!(auth.issue(fresh, None).is_err());
        let reopened = Auth::from_key(KEY).unwrap();
        reopened.open_users(&dir).unwrap();
        assert!(reopened.issue(fresh, None).is_err());
        assert!(reopened.issue(&keys[1], None).is_ok());
        assert!(
            reopened
                .add_user(NewUser {
                    id: "bootstrap".into(),
                    role: Role::Admin
                })
                .is_err()
        );
        drop(router);
        drop(auth);
        drop(reopened);
        for file in std::fs::read_dir(&dir).unwrap() {
            std::fs::remove_file(file.unwrap().path()).unwrap();
        }
        std::fs::remove_dir(dir).unwrap();
    }
    #[test]
    fn public_origin_requires_auth_and_secure_cookies() {
        assert!(
            Auth::disabled()
                .configure_origin("https://dashboard.example")
                .is_err()
        );
        let auth = Auth::from_key(KEY).unwrap();
        for origin in [
            "http://dashboard.example",
            "https://user:secret@dashboard.example",
            "https://dashboard.example/path",
            "https://dashboard.example?x=1",
        ] {
            assert!(auth.configure_origin(origin).is_err());
        }
        auth.configure_origin("https://dashboard.example:8443")
            .unwrap();
        assert!(app_cookie(&auth, "test", 60).contains("; Secure"));
    }
}
