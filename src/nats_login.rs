use crate::{App, connection, telemetry};
use axum::{
    extract::Request,
    http::{Method, StatusCode},
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio::sync::{RwLock, Semaphore};
use tower::ServiceExt;

#[derive(Clone, Copy, Default)]
pub struct Policy {
    pub enabled: bool,
    pub history: bool,
    pub monitoring: bool,
}
impl Policy {
    pub fn from_env() -> Result<Self, String> {
        fn flag(name: &str) -> Result<bool, String> {
            match std::env::var(name).as_deref() {
                Ok("1") => Ok(true),
                Err(std::env::VarError::NotPresent) | Ok("0") => Ok(false),
                _ => Err(format!("{name} must be 0 or 1")),
            }
        }
        let policy = Self {
            enabled: flag("NATSUI_NATS_LOGIN")?,
            history: flag("NATSUI_NATS_LOGIN_SHARED_HISTORY")?,
            monitoring: flag("NATSUI_NATS_LOGIN_SHARED_MONITORING")?,
        };
        if !policy.enabled && (policy.history || policy.monitoring) {
            return Err("Shared NATS login data requires NATSUI_NATS_LOGIN=1".into());
        }
        Ok(policy)
    }
}

// Session credentials remain in memory and never enter the profile configuration or SQLite.
pub struct Session {
    pub profile: String,
    pub username: String,
    pub connection: connection::Config,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Credentials {
    pub profile: String,
    username: String,
    password: String,
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
    static LOGIN_LIMIT: Semaphore = Semaphore::const_new(4);
    let Ok(_permit) = LOGIN_LIMIT.try_acquire() else {
        return StatusCode::TOO_MANY_REQUESTS.into_response();
    };
    let Some(config) = &app.connection else {
        return StatusCode::NOT_FOUND.into_response();
    };
    let Ok(connection) = config.for_user(&credentials.username, &credentials.password) else {
        return (
            StatusCode::BAD_REQUEST,
            "This profile does not support NATS password login.",
        )
            .into_response();
    };
    if connection.connect_user().await.is_err() {
        return (
            StatusCode::UNAUTHORIZED,
            "NATS login failed. Check credentials, connectivity and server authentication.",
        )
            .into_response();
    }
    match app.auth.nats_cookie(
        Session {
            profile: credentials.profile,
            username: credentials.username,
            connection,
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
            ["", "api", "history"]
            | ["", "api", "history", "window" | "backlog"]
            | ["", "api", "incidents"] => policy.history,
            [
                "",
                "api",
                "monitoring",
                "connections" | "subscriptions" | "routes",
            ]
            | ["", "api", "nodes", _, "connections"] => policy.monitoring,
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
                | "/api/config/preview"
                | "/api/config/apply"
                | "/api/operations/preview"
                | "/api/operations/apply"
        )
    } else {
        false
    }
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
    let state = if needs_inventory(&path) {
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
    app.current = Arc::new(RwLock::new(state));
    app.nats = Arc::new(RwLock::new(Some(client)));
    app.connection = None;
    app.demo = false;
    if !policy.monitoring {
        app.monitor = crate::monitoring::Monitor::new("").expect("empty monitoring configuration");
        *app.monitor.current.write().await = json!({"status":"restricted","nodes":[],"detail":"Shared monitoring is disabled for NATS login sessions."});
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
        value["dashboard"]["shared_history"] = json!(policy.history);
        value["dashboard"]["shared_monitoring"] = json!(policy.monitoring);
    }
    if !policy.monitoring {
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
