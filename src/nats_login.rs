use crate::{App, connection, telemetry};
use axum::{
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use base64::Engine;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::{Mutex, RwLock, Semaphore};
use tower::ServiceExt;

#[derive(Clone, Copy, Default)]
pub struct Policy {
    pub enabled: bool,
    pub history: bool,
    pub monitoring: bool,
    pub session_only: bool,
    pub certificates: bool,
}
impl Policy {
    pub fn from_env() -> Result<Self, String> {
        use crate::deployment::flag;
        let policy = Self {
            enabled: flag("NATSUI_NATS_LOGIN")?,
            history: flag("NATSUI_NATS_LOGIN_SHARED_HISTORY")?,
            monitoring: flag("NATSUI_NATS_LOGIN_SHARED_MONITORING")?,
            session_only: flag("NATSUI_NATS_LOGIN_SESSION_ONLY")?,
            certificates: flag("NATSUI_NATS_LOGIN_CERTIFICATES")?,
        };
        policy.validate()
    }
    fn validate(self) -> Result<Self, String> {
        let policy = self;
        if !policy.enabled
            && (policy.history || policy.monitoring || policy.session_only || policy.certificates)
        {
            return Err("NATS login options require NATSUI_NATS_LOGIN=1".into());
        }
        if policy.session_only && (policy.history || policy.monitoring) {
            return Err("Session-only mode cannot share collector history or monitoring".into());
        }
        Ok(policy)
    }
}

// Session credentials remain in memory and never enter the profile configuration or SQLite.
pub struct Session {
    pub profile: String,
    pub username: String,
    pub connection: connection::Config,
    pub observations: Option<Observations>,
    pub tools: crate::native_tools::SessionTools,
}

pub struct Observations {
    db: crate::store::Database,
    previous: Mutex<Option<telemetry::Snapshot>>,
}
impl Observations {
    fn new(db: crate::store::Database) -> Result<Self, String> {
        Ok(Self {
            db,
            previous: Mutex::new(None),
        })
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub profile: String,
    #[serde(default)]
    username: String,
    #[serde(default)]
    password: String,
    #[serde(default)]
    certificate: String,
    #[serde(default)]
    private_key: String,
}

pub async fn login(
    app: &App,
    headers: &axum::http::HeaderMap,
    credentials: Credentials,
) -> Response {
    if !app.auth.nats_policy().enabled || app.demo {
        return StatusCode::NOT_FOUND.into_response();
    }
    if let Err(status) = app.auth.login_attempt() {
        return (status, [(axum::http::header::RETRY_AFTER, "60")]).into_response();
    }
    static LOGIN_LIMIT: Semaphore = Semaphore::const_new(4);
    let Ok(_permit) = LOGIN_LIMIT.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let Some(config) = &app.connection else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let mut certificate_details = None;
    let certificate_login =
        !credentials.certificate.is_empty() || !credentials.private_key.is_empty();
    let (connection, username) = if certificate_login {
        if !app.auth.nats_policy().certificates {
            return (
                StatusCode::FORBIDDEN,
                "Client-certificate sign-in is disabled.",
            )
                .into_response();
        }
        if !credentials.username.is_empty()
            || !credentials.password.is_empty()
            || credentials.certificate.len() > 131072
            || credentials.private_key.len() > 32768
        {
            return (
                StatusCode::BAD_REQUEST,
                "Choose either a certificate and key or a username and password.",
            )
                .into_response();
        }
        let Ok(certificate) =
            base64::engine::general_purpose::STANDARD.decode(&credentials.certificate)
        else {
            return (StatusCode::BAD_REQUEST, "Invalid certificate encoding.").into_response();
        };
        certificate_details = match connection::Config::certificate_details(&certificate) {
            Ok(details) => Some(details),
            Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
        };
        match config.for_certificate(&certificate, credentials.private_key.as_bytes()) {
            Ok(identity) => identity,
            Err(error) => return (StatusCode::BAD_REQUEST, error).into_response(),
        }
    } else {
        if credentials.username.is_empty()
            || credentials.username.len() > 256
            || credentials.username.chars().any(char::is_control)
            || credentials.password.is_empty()
            || credentials.password.len() > 1024
        {
            return (
                StatusCode::BAD_REQUEST,
                "Enter a NATS username and password.",
            )
                .into_response();
        }
        match config.for_user(&credentials.username, &credentials.password) {
            Ok(connection) => (connection, credentials.username),
            Err(_) => {
                return (
                    StatusCode::BAD_REQUEST,
                    "This profile requires client-certificate sign-in.",
                )
                    .into_response();
            }
        }
    };
    if connection.connect_user().await.is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            "NATS login failed. Check credentials, connectivity and server authentication.",
        )
            .into_response();
    }
    let tools = match crate::native_tools::SessionTools::new(certificate_details) {
        Ok(tools) => tools,
        Err(error) => return (StatusCode::SERVICE_UNAVAILABLE, error).into_response(),
    };
    let observations = if app.auth.nats_policy().session_only {
        match Observations::new(tools.db.clone()) {
            Ok(observations) => Some(observations),
            Err(error) => return (StatusCode::SERVICE_UNAVAILABLE, error).into_response(),
        }
    } else {
        None
    };
    match app.auth.nats_cookie(
        Session {
            profile: credentials.profile,
            username,
            connection,
            observations,
            tools,
        },
        headers,
    ) {
        Ok(cookie) => (
            [(axum::http::header::SET_COOKIE, cookie)],
            StatusCode::NO_CONTENT,
        )
            .into_response(),
        Err(status) => status.into_response(),
    }
}

// This is a route boundary, not a translation of NATS permissions. New APIs are closed
// until their data source and credential path have been classified explicitly.
pub fn allowed(method: &Method, path: &str, policy: Policy) -> bool {
    let read = method == Method::GET || method == Method::HEAD;
    let parts: Vec<_> = path.split('/').collect();
    if read {
        match parts.as_slice() {
            ["", "api", "auth", "me"]
            | ["", "api", "profiles"]
            | ["", "api", "session"]
            | ["", "api", "snapshot"]
            | ["", "api", "settings"]
            | ["", "api", "editing"]
            | ["", "api", "config"]
            | ["", "api", "buckets"]
            | ["", "api", "buckets", _, _]
            | ["", "api", "buckets", _, _, "entry"]
            | ["", "api", "messages", _, _]
            | ["", "api", "records", _]
            | ["", "api", "latest", _] => true,
            ["", "api", "activity" | "watch"] => policy.session_only,
            ["", "api", "history"]
            | ["", "api", "history", "window" | "backlog"]
            | ["", "api", "incidents"] => policy.history || policy.session_only,
            [
                "",
                "api",
                "monitoring",
                "connections" | "subscriptions" | "routes",
            ]
            | ["", "api", "nodes", _, "connections"] => policy.monitoring || policy.session_only,
            _ => {
                path == "/"
                    || (!path.starts_with("/api/")
                        && (path.ends_with(".js")
                            || path.ends_with(".css")
                            || path.ends_with(".svg")))
            }
        }
    } else if method == Method::POST {
        matches!(
            path,
            "/api/auth/logout"
                | "/api/profiles/select"
                | "/api/session/monitoring"
                | "/api/config/preview"
                | "/api/config/apply"
                | "/api/operations/preview"
                | "/api/operations/apply"
        )
    } else {
        false
    }
}

// Count serialized bytes without allocating a second unbounded copy of the observation.
fn retainable(snapshot: &telemetry::Snapshot) -> bool {
    struct Budget(usize);
    impl std::io::Write for Budget {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.0 = self
                .0
                .checked_sub(bytes.len())
                .ok_or_else(|| std::io::Error::other("Observation cache limit"))?;
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    serde_json::to_writer(Budget(4 * 1024 * 1024), snapshot).is_ok()
}

fn url_page(query: &str) -> Result<usize, ()> {
    for (key, value) in reqwest::Url::parse(&format!("http://localhost/?{query}"))
        .map_err(|_| ())?
        .query_pairs()
    {
        if key == "page" {
            return value.parse().map_err(|_| ());
        }
    }
    Ok(0)
}
fn needs_inventory(path: &str) -> bool {
    matches!(path, "/api/snapshot" | "/api/buckets")
}

pub async fn dispatch(mut app: App, session: Arc<Session>, request: Request) -> Response {
    let path = request.uri().path().to_owned();
    let policy = app.auth.nats_policy();
    if !allowed(request.method(), &path, policy) {
        return (
            StatusCode::FORBIDDEN,
            "This endpoint is not shared with NATS login sessions.",
        )
            .into_response();
    }
    static LIMIT: OnceLock<Semaphore> = OnceLock::new();
    let Ok(_permit) = LIMIT.get_or_init(|| Semaphore::new(32)).try_acquire() else {
        return (
            StatusCode::TOO_MANY_REQUESTS,
            "NATS request capacity reached. Retry shortly.",
        )
            .into_response();
    };
    // Fresh authentication bounds credential revocation to the next HTTP request.
    // Collector clients, cached inventories and reconnect credentials are never reused.
    let client = match session.connection.connect_user().await {
        Ok(client) => client,
        Err(_) => return (StatusCode::BAD_GATEWAY, "NATS authentication or connectivity is unavailable. No collector credentials were used.").into_response(),
    };
    let monitoring_choice = *session.tools.monitoring.lock().await;
    let session_monitoring = policy.session_only && monitoring_choice.enabled;
    if policy.session_only
        && (path.starts_with("/api/monitoring/") || path.starts_with("/api/nodes/"))
    {
        if !session_monitoring {
            return (StatusCode::FORBIDDEN, "Session NATS monitoring is disabled").into_response();
        }
        let params = request.uri().query().unwrap_or("");
        let page = url_page(params);
        let Ok(page) = page else {
            return (StatusCode::BAD_REQUEST, "Invalid inventory page").into_response();
        };
        let parts: Vec<_> = path.split('/').collect();
        let result = if let ["", "api", "nodes", index, "connections"] = parts.as_slice() {
            let slots = session.tools.servers.lock().await;
            let server = index
                .parse::<usize>()
                .ok()
                .and_then(|index| slots.get(index).cloned());
            drop(slots);
            match server {
                Some(server) => {
                    crate::native_tools::inventory(
                        &client,
                        &session.tools,
                        "connections",
                        0,
                        Some(&server),
                    )
                    .await
                }
                None => Err((
                    StatusCode::NOT_FOUND,
                    "Node has not been observed in this session".into(),
                )),
            }
        } else {
            crate::native_tools::inventory(
                &client,
                &session.tools,
                parts.last().unwrap_or(&""),
                page,
                None,
            )
            .await
        };
        return match result {
            Ok(value) => axum::Json(value).into_response(),
            Err(error) => error.into_response(),
        };
    }
    // Serializing observations avoids duplicate or out-of-order samples from multiple tabs.
    let mut previous = if path == "/api/snapshot" {
        match &session.observations {
            Some(observations) => Some(observations.previous.lock().await),
            None => None,
        }
    } else {
        None
    };
    let cached = previous
        .as_ref()
        .and_then(|previous| previous.as_ref())
        .filter(|state| telemetry::now().saturating_sub(state.observed_at) < 2)
        .cloned();
    let mut state = if let Some(state) = cached.clone() {
        state
    } else if needs_inventory(&path) {
        tokio::time::timeout(
            Duration::from_secs(20),
            telemetry::observe(&client, &app.prefix, &app.scope),
        )
        .await
        .unwrap_or_else(|_| {
            telemetry::Snapshot::unavailable("Collection exceeded its time budget", &app.scope)
        })
    } else {
        telemetry::Snapshot::unavailable(
            "Inventory is collected with the logged-in NATS credentials",
            &app.scope,
        )
    };
    let mut monitoring = if session_monitoring && path == "/api/snapshot" {
        crate::native_tools::observe(&client, &session.tools).await
    } else if policy.session_only {
        json!({"status":"restricted","source":"session_nats","enabled":session_monitoring,"nodes":[],"detail":"Session NATS monitoring is disabled. Enable monitoring through NATS on the Nodes page."})
    } else {
        json!({"status":"restricted","nodes":[],"detail":"Collector monitoring is not shared with this login."})
    };
    if policy.session_only {
        monitoring["revision"] = json!(monitoring_choice.revision);
    }
    if let Some(observations) = &session.observations {
        app.db = observations.db.clone();
        app.settings_cache = Arc::new(RwLock::new(crate::store::Settings::default()));
        if let Some(previous) = previous.as_mut()
            && cached.is_none()
        {
            if let Some(old) = previous.as_ref() {
                let changes = crate::incidents::changes(
                    old,
                    &state,
                    &monitoring,
                    &monitoring,
                    crate::store::Settings::default().backlog_threshold,
                );
                if app.db.incidents(&app.scope, false, changes).await.is_err() {
                    return (
                        StatusCode::SERVICE_UNAVAILABLE,
                        "Session incident storage is unavailable",
                    )
                        .into_response();
                }
            }
            if app.db.sample_resources(&state, &monitoring).await.is_err() {
                return (
                    StatusCode::SERVICE_UNAVAILABLE,
                    "Session history storage is unavailable",
                )
                    .into_response();
            }
            if retainable(&state) {
                **previous = Some(state.clone());
            } else {
                **previous = None;
                state.issues.push("Session change comparison is unavailable because the observation exceeds its 4 MiB cache budget".into());
            }
        }
    }
    drop(previous);
    app.current = Arc::new(RwLock::new(state));
    app.nats = Arc::new(RwLock::new(Some(client)));
    app.connection = None;
    app.demo = false;
    if !policy.monitoring {
        app.monitor = crate::monitoring::Monitor::new("").expect("empty monitoring configuration");
        *app.monitor.current.write().await = monitoring;
    }
    let response = match crate::application_routes(app).oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    };
    if response.status().is_success()
        && (path == "/api/snapshot" || path.starts_with("/api/history") || path == "/api/incidents")
    {
        return scope_response(response, &path, policy).await;
    }
    response
}

fn scope_value(value: &mut Value, path: &str, policy: Policy) {
    if path == "/api/snapshot" {
        value["dashboard"]["login_method"] = json!("nats");
        value["dashboard"]["session_history"] = json!(policy.session_only);
        value["dashboard"]["shared_history"] = json!(policy.history);
        value["dashboard"]["shared_monitoring"] = json!(policy.monitoring);
    }
    if !policy.monitoring && !policy.session_only {
        let rows = if path == "/api/history" {
            value.as_array_mut()
        } else if path.starts_with("/api/history/") {
            value.get_mut("samples").and_then(Value::as_array_mut)
        } else {
            None
        };
        if let Some(rows) = rows {
            for sample in rows {
                if let Some(resources) = sample
                    .pointer_mut("/summary/resources")
                    .and_then(Value::as_object_mut)
                {
                    resources.remove("monitoring");
                }
            }
        }
        if path == "/api/incidents"
            && let Some(rows) = value.as_array_mut()
        {
            rows.retain(|row| matches!(row["resource"].as_str(), Some("stream" | "consumer")));
        }
    }
}

async fn scope_response(response: Response, path: &str, policy: Policy) -> Response {
    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, 24_000_000).await else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    let Ok(mut value) = serde_json::from_slice::<Value>(&bytes) else {
        return StatusCode::BAD_GATEWAY.into_response();
    };
    scope_value(&mut value, path, policy);
    parts.headers.remove(axum::http::header::CONTENT_LENGTH);
    Response::from_parts(parts, axum::body::Body::from(value.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn observation_cache_is_bounded() {
        let mut snapshot = telemetry::demo("bounded");
        assert!(retainable(&snapshot));
        snapshot.streams[0]["config"]["description"] = json!("x".repeat(4 * 1024 * 1024));
        assert!(!retainable(&snapshot));
    }
    #[test]
    fn session_policy_requires_login_and_rejects_sharing() {
        assert!(
            Policy {
                session_only: true,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        assert!(
            Policy {
                certificates: true,
                ..Default::default()
            }
            .validate()
            .is_err()
        );
        let policy = Policy {
            enabled: true,
            session_only: true,
            ..Default::default()
        };
        assert!(policy.validate().is_ok());
        assert!(
            Policy {
                history: true,
                ..policy
            }
            .validate()
            .is_err()
        );
        assert!(
            Policy {
                monitoring: true,
                ..policy
            }
            .validate()
            .is_err()
        );
        assert!(allowed(&Method::GET, "/api/history", policy));
        assert!(allowed(&Method::GET, "/api/monitoring/routes", policy));
    }
    #[test]
    fn session_routes_fail_closed_and_sharing_is_explicit() {
        let private = Policy {
            enabled: true,
            ..Policy::default()
        };
        for path in [
            "/api/users",
            "/api/managed",
            "/api/nats-users",
            "/api/activity",
            "/api/events",
            "/api/future-api",
            "/api/history",
            "/api/history/window",
            "/api/incidents",
            "/api/monitoring/routes",
            "/api/nodes/0/connections",
        ] {
            assert!(!allowed(&Method::GET, path, private), "{path}");
        }
        for path in [
            "/api/settings",
            "/api/users",
            "/api/managed/a/apply",
            "/api/nats-users/a/apply",
            "/api/config/apply/extra",
        ] {
            assert!(!allowed(&Method::POST, path, private), "{path}");
        }
        for path in [
            "/api/snapshot",
            "/api/config",
            "/api/buckets/kv/ORDERS/entry",
            "/api/messages/ORDERS/1",
            "/api/auth/me",
        ] {
            assert!(allowed(&Method::GET, path, private), "{path}");
        }
        assert!(allowed(&Method::POST, "/api/operations/apply", private));
        assert!(allowed(
            &Method::GET,
            "/api/history",
            Policy {
                history: true,
                ..private
            }
        ));
        assert!(!allowed(
            &Method::GET,
            "/api/monitoring/routes",
            Policy {
                history: true,
                ..private
            }
        ));
        assert!(allowed(
            &Method::GET,
            "/api/monitoring/routes",
            Policy {
                monitoring: true,
                ..private
            }
        ));
        assert!(!allowed(
            &Method::GET,
            "/api/history",
            Policy {
                monitoring: true,
                ..private
            }
        ));
    }
    #[test]
    fn history_sharing_does_not_share_monitoring() {
        let policy = Policy {
            enabled: true,
            history: true,
            monitoring: false,
            ..Default::default()
        };
        let row = json!({"summary":{"resources":{"streams":[{"name":"A"}],"monitoring":{"nodes":[{"server_name":"secret"}]}}}});
        let mut value = json!([row.clone()]);
        scope_value(&mut value, "/api/history", policy);
        assert!(value[0]["summary"]["resources"].get("monitoring").is_none());
        assert_eq!(value[0]["summary"]["resources"]["streams"][0]["name"], "A");
        let mut value = json!({"samples":[row]});
        scope_value(&mut value, "/api/history/window", policy);
        assert!(
            value["samples"][0]["summary"]["resources"]
                .get("monitoring")
                .is_none()
        );
        let mut value = json!([{"resource":"node"},{"resource":"stream"},{"resource":"unknown"}]);
        scope_value(&mut value, "/api/incidents", policy);
        assert_eq!(value, json!([{"resource":"stream"}]));
    }
}
