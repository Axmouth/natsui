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
pub(crate) struct Profile {
    pub(crate) id: String,
    pub(crate) name: String,
    pub(crate) urls: Vec<String>,
    pub(crate) credentials: Option<PathBuf>,
    pub(crate) ca: Option<PathBuf>,
    pub(crate) certificate: Option<PathBuf>,
    pub(crate) key: Option<PathBuf>,
    #[serde(default)]
    pub(crate) tls: bool,
    #[serde(default)]
    pub(crate) domain: String,
    #[serde(default)]
    pub(crate) accounts: Vec<String>,
    #[serde(default)]
    pub(crate) monitor_urls: Vec<String>,
    pub(crate) monitor_config_file: Option<String>,
    #[serde(default)]
    pub(crate) allow_writes: bool,
}
impl Profile {
    pub(crate) fn monitor(
        &self,
        session_only: bool,
    ) -> Result<monitoring::Monitor, Box<dyn std::error::Error>> {
        if session_only {
            monitoring::Monitor::new("")
        } else if let Some(path) = &self.monitor_config_file {
            if !self.monitor_urls.is_empty() {
                return Err(
                    "Each profile must choose monitoring URLs or a monitoring config file".into(),
                );
            }
            monitoring::Monitor::from_file(path)
        } else {
            monitoring::Monitor::new(&self.monitor_urls.join(","))
        }
    }
    pub(crate) fn connection(&self) -> connection::Config {
        connection::Config {
            url: self.urls.join(","),
            credentials: self.credentials.clone(),
            ca: self.ca.clone(),
            certificate: self.certificate.clone(),
            key: self.key.clone(),
            tls: self.tls,
            session_tls: None,
        }
    }
}
pub(crate) fn definitions() -> Result<Vec<Profile>, Box<dyn std::error::Error>> {
    let Some(path) = std::env::var_os("NATSUI_PROFILES_FILE") else {
        return Ok(vec![]);
    };
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
    let mut ids = std::collections::BTreeSet::from(["default".to_owned()]);
    for profile in &config.profiles {
        if !valid_id(&profile.id)
            || !ids.insert(profile.id.clone())
            || profile.name.is_empty()
            || profile.name.len() > 128
            || (!profile.domain.is_empty() && !telemetry::valid_token(&profile.domain))
            || crate::deployment::parse_accounts(&profile.accounts.join(","))
                .map_or(true, |parsed| parsed.len() != profile.accounts.len())
        {
            return Err("Invalid or duplicate profile identity".into());
        }
    }
    Ok(config.profiles)
}

#[derive(Clone)]
struct Registry {
    auth: auth::Auth,
    routes: Arc<BTreeMap<String, (String, Router)>>,
    apps: Arc<BTreeMap<String, App>>,
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
pub async fn router(
    base: App,
    directory: Option<&str>,
) -> Result<Router, Box<dyn std::error::Error>> {
    let mut contexts = BTreeMap::new();
    contexts.insert("default".to_owned(), base.clone());
    let mut apps = BTreeMap::new();
    apps.insert(
        "default".into(),
        (base.scope.clone(), crate::application_routes(base.clone())),
    );
    {
        for profile in definitions()? {
            let connection = profile.connection();
            connection.validate()?;
            if base.auth.nats_policy().session_only {
                connection.validate_session_only()?;
            }
            connection.options().await?;
            let binding = connection.binding(&profile.domain)?;
            let db = match directory {
                Some(directory) => {
                    let directory = PathBuf::from(directory).join("profiles").join(&profile.id);
                    store::Database::open(directory.to_str().ok_or("Invalid profile directory")?)?
                }
                None => store::Database::memory()?,
            };
            db.bind_profile(&profile.name, false, &binding, false)
                .await?;
            let monitor = profile.monitor(base.auth.nats_policy().session_only)?;
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
                accounts: Arc::new(profile.accounts.clone()),
                connection: Some(connection),
            };
            if !base.auth.nats_policy().session_only {
                tokio::spawn(app.monitor.clone().collect());
                tokio::spawn(crate::collect(app.clone()));
                tokio::spawn(crate::incidents::collect(app.clone()));
            }
            contexts.insert(profile.id.clone(), app.clone());
            apps.insert(profile.id, (profile.name, crate::application_routes(app)));
        }
    }
    if base.auth.nats_policy().enabled {
        for app in contexts.values() {
            if app.demo {
                return Err("NATS login cannot be used with the simulated demo".into());
            }
            if base.auth.nats_policy().session_only {
                app.connection
                    .as_ref()
                    .ok_or("Missing connection profile")?
                    .validate_session_only()?;
            }
            if !base.auth.nats_policy().certificates {
                app.connection
                    .as_ref()
                    .ok_or("NATS login requires a connection profile")?
                    .for_user("validation", "validation")?;
            }
        }
    }
    let registry = Registry {
        auth: base.auth.clone(),
        routes: Arc::new(apps),
        apps: Arc::new(contexts),
    };
    Ok(Router::new()
        .route(
            "/api/auth/nats",
            get(login_options)
                .post(nats_login)
                .layer(axum::extract::DefaultBodyLimit::max(196608)),
        )
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
async fn login_options(State(registry): State<Registry>) -> Json<Value> {
    let policy = registry.auth.nats_policy();
    Json(
        json!({"enabled":policy.enabled,"certificates":policy.certificates,"session_only":policy.session_only,"shared_history":policy.history,"shared_monitoring":policy.monitoring,
        "profiles":if policy.enabled { registry.routes.iter().map(|(id,(name,_))| json!({"id":id,"name":name})).collect::<Vec<_>>() } else { vec![] }}),
    )
}
async fn nats_login(
    State(registry): State<Registry>,
    headers: HeaderMap,
    Json(credentials): Json<crate::nats_login::Credentials>,
) -> Response {
    if !registry.auth.nats_policy().enabled {
        return StatusCode::NOT_FOUND.into_response();
    }
    let Some(app) = registry.apps.get(&credentials.profile) else {
        return StatusCode::BAD_REQUEST.into_response();
    };
    crate::nats_login::login(app, &headers, credentials).await
}
async fn list(State(registry): State<Registry>, headers: HeaderMap) -> Json<Value> {
    Json(
        json!({"selected":registry.auth.nats_session(&headers).map(|s|s.profile.clone()).unwrap_or_else(||selected(&headers)),"profiles":registry.routes.iter().filter(|(id,_)|registry.auth.allowed_profile(&headers,id)).map(|(id,(name,_))|json!({"id":id,"name":name})).collect::<Vec<_>>() }),
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
    if scoped
        && let Some(session) = request
            .extensions()
            .get::<Arc<crate::nats_login::Session>>()
            .cloned()
    {
        let Some(app) = registry.apps.get(&id) else {
            return StatusCode::NOT_FOUND.into_response();
        };
        return crate::nats_login::dispatch(app.clone(), session, request).await;
    }
    if scoped && registry.auth.nats_policy().session_only {
        return (
            StatusCode::FORBIDDEN,
            "Sign in with NATS to access session data.",
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
    async fn concurrent_session_replacement_never_reaches_collector() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        use tokio::sync::Notify;
        let auth = auth::Auth::with_nats_policy(crate::nats_login::Policy {
            enabled: true,
            ..Default::default()
        });
        let identity = || crate::nats_login::Session {
            profile: "default".into(),
            username: "reader".into(),
            access: None,
            observations: None,
            tools: crate::native_tools::SessionTools::new(None).unwrap(),
            connection: connection::Config {
                url: "nats://127.0.0.1:1".into(),
                credentials: None,
                ca: None,
                certificate: None,
                key: None,
                tls: false,
                session_tls: None,
            },
        };
        let cookie = auth.nats_cookie(identity(), &HeaderMap::new()).unwrap();
        let cookie = cookie.split(';').next().unwrap().to_owned();
        let hits = Arc::new(AtomicUsize::new(0));
        let counter = hits.clone();
        let collector = Router::new().fallback(move || {
            let counter = counter.clone();
            async move {
                counter.fetch_add(1, Ordering::SeqCst);
                "collector data"
            }
        });
        let registry = Registry {
            auth: auth.clone(),
            routes: Arc::new(BTreeMap::from([(
                "default".into(),
                ("Default".into(), collector),
            )])),
            apps: Arc::new(BTreeMap::new()),
        };
        let entered = Arc::new(Notify::new());
        let release = Arc::new(Notify::new());
        let notify_entered = entered.clone();
        let notify_release = release.clone();
        let router = Router::new()
            .fallback(dispatch)
            .with_state(registry)
            .layer(axum::middleware::from_fn(
                move |request: Request, next: axum::middleware::Next| {
                    let entered = notify_entered.clone();
                    let release = notify_release.clone();
                    async move {
                        assert!(
                            request
                                .extensions()
                                .get::<Arc<crate::nats_login::Session>>()
                                .is_some()
                        );
                        entered.notify_one();
                        release.notified().await;
                        next.run(request).await
                    }
                },
            ))
            .layer(axum::middleware::from_fn_with_state(
                auth.clone(),
                auth::guard,
            ));
        let request = || {
            Request::builder()
                .uri("/api/snapshot")
                .header("cookie", &cookie)
                .body(Body::empty())
                .unwrap()
        };
        let in_flight = tokio::spawn(router.clone().oneshot(request()));
        tokio::time::timeout(std::time::Duration::from_secs(3), entered.notified())
            .await
            .unwrap();
        let mut headers = HeaderMap::new();
        headers.insert("cookie", cookie.parse().unwrap());
        auth.nats_cookie(identity(), &headers).unwrap();
        release.notify_one();
        let response = tokio::time::timeout(std::time::Duration::from_secs(3), in_flight)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(response.status(), StatusCode::FORBIDDEN);
        assert_eq!(hits.load(Ordering::SeqCst), 0);
        let response =
            tokio::time::timeout(std::time::Duration::from_secs(3), router.oneshot(request()))
                .await
                .unwrap()
                .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(hits.load(Ordering::SeqCst), 0);
    }
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
            apps: Arc::new(BTreeMap::new()),
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
