use crate::{App, auth, connection, monitoring, store, telemetry};
use axum::{
    Json, Router,
    extract::{Request, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read, path::PathBuf, sync::Arc};
use tokio::sync::RwLock;
use tower::ServiceExt;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Configuration {
    profiles: Vec<Profile>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Profile {
    id: String,
    name: String,
    urls: Vec<String>,
    credentials: Option<PathBuf>,
    ca: Option<PathBuf>,
    certificate: Option<PathBuf>,
    key: Option<PathBuf>,
    #[serde(default)]
    tls: bool,
    #[serde(default)]
    domain: String,
    #[serde(default)]
    monitor_urls: Vec<String>,
    monitor_config_file: Option<String>,
    #[serde(default)]
    allow_writes: bool,
}
#[derive(Clone)]
struct Registry {
    auth: auth::Auth,
    routes: Arc<BTreeMap<String, (String, Router)>>,
}
fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 48
        && id
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-' || b == b'_')
}
fn selected(headers: &HeaderMap) -> String {
    headers
        .get("x-natsui-profile")
        .and_then(|value| value.to_str().ok())
        .unwrap_or("default")
        .to_owned()
}
pub async fn router(base: App, directory: &str) -> Result<Router, Box<dyn std::error::Error>> {
    let mut apps = BTreeMap::new();
    apps.insert(
        "default".into(),
        (base.scope.clone(), crate::application_routes(base.clone())),
    );
    if let Some(path) = std::env::var_os("NATSUI_PROFILES_FILE") {
        let mut bytes = Vec::new();
        std::fs::File::open(path)?
            .take(65537)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            return Err("Profile configuration exceeds 64 KiB".into());
        }
        let config: Configuration =
            serde_json::from_slice(&bytes).map_err(|_| "Invalid profile configuration")?;
        if config.profiles.len() > 7 {
            return Err("At most seven additional profiles are supported".into());
        }
        for profile in config.profiles {
            if !valid_id(&profile.id)
                || apps.contains_key(&profile.id)
                || profile.name.is_empty()
                || profile.name.len() > 128
                || (!profile.domain.is_empty() && !telemetry::valid_token(&profile.domain))
            {
                return Err("Invalid or duplicate profile identity".into());
            }
            let connection = connection::Config {
                url: profile.urls.join(","),
                credentials: profile.credentials,
                ca: profile.ca,
                certificate: profile.certificate,
                key: profile.key,
                tls: profile.tls,
            };
            connection.options().await?;
            let binding = connection.binding(&profile.domain)?;
            let directory = PathBuf::from(directory).join("profiles").join(&profile.id);
            let db = store::Database::open(directory.to_str().ok_or("Invalid profile directory")?)?;
            db.bind_profile(&profile.name, false, &binding, false)
                .await?;
            let monitor = if let Some(path) = profile.monitor_config_file {
                if !profile.monitor_urls.is_empty() {
                    return Err(
                        "Each profile must choose monitoring URLs or a monitoring config file"
                            .into(),
                    );
                }
                monitoring::Monitor::from_file(&path)?
            } else {
                monitoring::Monitor::new(&profile.monitor_urls.join(","))?
            };
            let app = App {
                auth: base.auth.clone(),
                editor: crate::editing::Editor::new(profile.allow_writes),
                settings_cache: Arc::new(RwLock::new(db.settings().await?)),
                db,
                current: Arc::new(RwLock::new(telemetry::Snapshot::unavailable(
                    "Connecting to NATS",
                    &profile.name,
                ))),
                nats: Arc::new(RwLock::new(None)),
                monitor,
                demo: false,
                prefix: if profile.domain.is_empty() {
                    "$JS.API".into()
                } else {
                    format!("$JS.{}.API", profile.domain)
                },
                scope: profile.name.clone(),
                connection: Some(connection),
            };
            tokio::spawn(app.monitor.clone().collect());
            tokio::spawn(crate::collect(app.clone()));
            tokio::spawn(crate::incidents::collect(app.clone()));
            apps.insert(profile.id, (profile.name, crate::application_routes(app)));
        }
    }
    let registry = Registry {
        auth: base.auth.clone(),
        routes: Arc::new(apps),
    };
    Ok(Router::new()
        .route("/api/profiles", get(list))
        .route(
            "/api/profiles/select",
            axum::routing::post(select).layer(axum::extract::DefaultBodyLimit::max(1024)),
        )
        .fallback(dispatch)
        .layer(axum::middleware::from_fn_with_state(
            base.auth.clone(),
            auth::guard,
        ))
        .layer(axum::middleware::from_fn_with_state(
            base.auth,
            crate::network_request,
        ))
        .with_state(registry))
}
async fn list(State(registry): State<Registry>, headers: HeaderMap) -> Json<Value> {
    Json(
        json!({"selected":selected(&headers),"profiles":registry.routes.iter().filter(|(id,_)|registry.auth.allowed_profile(&headers,id)).map(|(id,(name,_))|json!({"id":id,"name":name})).collect::<Vec<_>>() }),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    id: String,
}
async fn select(
    State(registry): State<Registry>,
    headers: HeaderMap,
    Json(selection): Json<Selection>,
) -> Response {
    if !registry.routes.contains_key(&selection.id) {
        return StatusCode::NOT_FOUND.into_response();
    }
    if !registry.auth.allowed_profile(&headers, &selection.id) {
        return StatusCode::FORBIDDEN.into_response();
    }
    StatusCode::NO_CONTENT.into_response()
}
async fn dispatch(State(registry): State<Registry>, request: Request) -> Response {
    let path = request.uri().path();
    let scoped = path.starts_with("/api/")
        && ![
            "/api/auth/",
            "/api/users",
            "/api/managed",
            "/api/nats-users",
        ]
        .iter()
        .any(|prefix| path.starts_with(prefix));
    let id = if scoped {
        selected(request.headers())
    } else {
        "default".into()
    };
    let Some((_, route)) = registry.routes.get(&id) else {
        return (
            StatusCode::CONFLICT,
            "Selected profile is no longer configured. Choose another profile.",
        )
            .into_response();
    };
    if scoped && !registry.auth.allowed_profile(request.headers(), &id) {
        return (
            StatusCode::FORBIDDEN,
            "Access to this profile is not granted.",
        )
            .into_response();
    }
    match route.clone().oneshot(request).await {
        Ok(response) => response,
        Err(never) => match never {},
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    #[tokio::test]
    async fn profiles_route_to_explicit_context_and_reject_unknown_selection() {
        let routes = BTreeMap::from([
            (
                "default".into(),
                (
                    "Default".into(),
                    Router::new().fallback(|| async { "first" }),
                ),
            ),
            (
                "stage".into(),
                (
                    "Stage".into(),
                    Router::new().fallback(|| async { "second" }),
                ),
            ),
        ]);
        let registry = Registry {
            auth: auth::Auth::disabled(),
            routes: Arc::new(routes),
        };
        for (profile, expected) in [
            ("stage", "second"),
            ("default", "first"),
            ("stage", "second"),
        ] {
            let request = Request::builder()
                .uri("/api/snapshot")
                .header("x-natsui-profile", profile)
                .body(Body::empty())
                .unwrap();
            let response = dispatch(State(registry.clone()), request).await;
            assert_eq!(
                &axum::body::to_bytes(response.into_body(), 1024)
                    .await
                    .unwrap()[..],
                expected.as_bytes()
            );
        }
        assert_eq!(
            select(
                State(registry.clone()),
                HeaderMap::new(),
                Json(Selection {
                    id: "unknown".into()
                })
            )
            .await
            .status(),
            StatusCode::NOT_FOUND
        );
        assert_eq!(
            select(
                State(registry.clone()),
                HeaderMap::new(),
                Json(Selection { id: "stage".into() })
            )
            .await
            .status(),
            StatusCode::NO_CONTENT
        );
        let missing = Request::builder()
            .uri("/api/snapshot")
            .header("x-natsui-profile", "removed")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            dispatch(State(registry.clone()), missing).await.status(),
            StatusCode::CONFLICT
        );
        let shell = Request::builder()
            .uri("/")
            .header("x-natsui-profile", "removed")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            dispatch(State(registry), shell).await.status(),
            StatusCode::OK
        );
        for id in ["../escape", "a.b", "", "default/path"] {
            assert!(!valid_id(id));
        }
    }
}
