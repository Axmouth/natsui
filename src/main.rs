mod connection;
mod incidents;
mod messages;
mod monitoring;
mod resources;
#[cfg(test)]
mod security_tests;
mod simulation;
mod store;
mod telemetry;

use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, StatusCode, header},
    response::{Html, IntoResponse, Response},
    routing::get,
};
use serde_json::{Value, json};
use std::{
    net::{IpAddr, SocketAddr},
    sync::Arc,
    time::Duration,
};
use telemetry::Snapshot;
use tokio::sync::RwLock;

#[derive(Clone)]
struct App {
    db: store::Database,
    current: Arc<RwLock<Snapshot>>,
    nats: Arc<RwLock<Option<async_nats::Client>>>,
    monitor: monitoring::Monitor,
    demo: bool,
    prefix: String,
    scope: String,
    connection: Option<connection::Config>,
    settings_cache: Arc<RwLock<store::Settings>>,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let demo = std::env::args().any(|a| a == "--demo");
    let port: u16 = std::env::var("NATSUI_PORT")
        .unwrap_or("4321".into())
        .parse()?;
    let bind = match std::env::var("NATSUI_CONTAINER").as_deref() {
        Ok("1") => [0, 0, 0, 0],
        Err(_) | Ok("0") => [127, 0, 0, 1],
        _ => return Err("NATSUI_CONTAINER must be 0 or 1".into()),
    };
    let address = SocketAddr::new(IpAddr::from(bind), port);
    let domain = std::env::var("NATSUI_DOMAIN").unwrap_or_default();
    if !domain.is_empty() && !telemetry::valid_token(&domain) {
        return Err("Invalid JetStream domain".into());
    }
    let scope = std::env::var("NATSUI_PROFILE").unwrap_or("Local NATS".into());
    let dir = std::env::var("NATSUI_DATA_DIR").unwrap_or("data".into());
    let db = store::Database::open(&dir)?;
    let settings_cache = Arc::new(RwLock::new(db.settings().await?));
    let connection = if demo {
        None
    } else {
        Some(connection::Config::from_env()?)
    };
    let binding = if let Some(config) = &connection {
        config.options().await?;
        config.binding(&domain)?
    } else {
        "simulation-v1".into()
    };
    db.bind_profile(
        &scope,
        demo,
        &binding,
        std::env::var("NATSUI_ADOPT_LEGACY_PROFILE").as_deref() == Ok("1"),
    )
    .await?;
    let monitor =
        monitoring::Monitor::new(&std::env::var("NATSUI_MONITOR_URLS").unwrap_or_default())?;
    if !demo {
        tokio::spawn(monitor.clone().collect());
    }
    let app = App {
        connection,
        settings_cache,
        monitor,
        db,
        current: Arc::new(RwLock::new(Snapshot::unavailable(
            "Connecting to NATS",
            &scope,
        ))),
        nats: Arc::new(RwLock::new(None)),
        demo,
        prefix: if domain.is_empty() {
            "$JS.API".into()
        } else {
            format!("$JS.{domain}.API")
        },
        scope,
    };
    tokio::spawn(collect(app.clone()));
    tokio::spawn(incidents::collect(app.clone()));
    let router = router(app);
    let listener = tokio::net::TcpListener::bind(address).await?;
    println!(
        "natsui: http://{address} ({})",
        if demo {
            "DEMO simulation"
        } else {
            "live, read-only NATS"
        }
    );
    axum::serve(listener, router)
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}

fn router(app: App) -> Router {
    Router::new()
        .route(
            "/",
            get(|| async { Html(include_str!("../web/index.html")) }),
        )
        .route(
            "/app.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/app.js"),
                )
            }),
        )
        .route(
            "/style.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css")],
                    include_str!("../web/style.css"),
                )
            }),
        )
        .route(
            "/fibril.css",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/css")],
                    include_str!("../web/fibril.css"),
                )
            }),
        )
        .route(
            "/kitten.svg",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "image/svg+xml")],
                    include_str!("../web/kitten.svg"),
                )
            }),
        )
        .route(
            "/demo.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/demo.js"),
                )
            }),
        )
        .route(
            "/trends.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/trends.js"),
                )
            }),
        )
        .route("/api/nodes/{index}/connections", get(node_connections))
        .route("/api/snapshot", get(snapshot))
        .route("/api/history", get(history))
        .route("/api/history/window", get(history_window))
        .route("/api/activity", get(activity))
        .route("/api/incidents", get(incident_history))
        .route("/api/monitoring/{kind}", get(monitor_inventory))
        .route("/api/settings", get(settings).put(save_settings))
        .route("/api/messages/{stream}/{sequence}", get(messages::message))
        .route("/api/records/{stream}", get(messages::browse))
        .route("/api/latest/{stream}", get(messages::latest))
        .route(
            "/subjects.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/subjects.js"),
                )
            }),
        )
        .route(
            "/workspace.js",
            get(|| async {
                (
                    [(header::CONTENT_TYPE, "text/javascript")],
                    include_str!("../web/workspace.js"),
                )
            }),
        )
        .route("/healthz", get(|| async { "ok" }))
        .route("/readyz", get(readiness))
        .layer(axum::middleware::from_fn(local_request))
        .with_state(app)
}

// Loopback binding or a loopback-only container port is the access boundary. Host checks also reject DNS
// rebinding; mutations require a non-simple header and no cross-origin access.
async fn local_request(request: axum::extract::Request, next: axum::middleware::Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|h| h.to_str().ok())
        .unwrap_or("");
    let name = host.split(':').next().unwrap_or("");
    if name != "127.0.0.1" && name != "localhost" {
        return StatusCode::FORBIDDEN.into_response();
    }
    if let Some(origin) = request.headers().get(header::ORIGIN)
        && origin.to_str().ok() != Some(format!("http://{host}").as_str())
    {
        return StatusCode::FORBIDDEN.into_response();
    }
    let mut response = next.run(request).await;
    response
        .headers_mut()
        .insert(header::CACHE_CONTROL, "no-store".parse().unwrap());
    response
        .headers_mut()
        .insert("x-content-type-options", "nosniff".parse().unwrap());
    response.headers_mut().insert("content-security-policy", "default-src 'self'; script-src 'self'; style-src 'self'; img-src 'self'; connect-src 'self'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'".parse().unwrap());
    response
}

async fn snapshot(State(app): State<App>) -> Json<Value> {
    let state = app.current.read().await.clone();
    let settings = match app.db.settings().await {
        Ok(settings) => {
            *app.settings_cache.write().await = settings.clone();
            settings
        }
        Err(_) => app.settings_cache.read().await.clone(),
    };
    Json(
        json!({"snapshot": state, "summary": telemetry::summarize(&state, settings.backlog_threshold), "settings": settings, "monitoring": app.monitor.current.read().await.clone(), "dashboard":{"version":env!("CARGO_PKG_VERSION"),"storage":app.db.health()}}),
    )
}
async fn readiness(State(app): State<App>) -> (StatusCode, Json<Value>) {
    let state = app.current.read().await;
    let health = app.db.health();
    let interval = app.settings_cache.read().await.refresh_seconds;
    let fresh = telemetry::now().saturating_sub(state.observed_at) <= interval * 3 + 20;
    let ready = state.status == "complete" && fresh && health["status"] == "ok";
    (
        if ready {
            StatusCode::OK
        } else {
            StatusCode::SERVICE_UNAVAILABLE
        },
        Json(
            json!({"ready":ready,"collection":state.status,"fresh":fresh,"storage":health,"version":env!("CARGO_PKG_VERSION")}),
        ),
    )
}
async fn node_connections(
    State(app): State<App>,
    Path(index): Path<usize>,
) -> Result<Json<Value>, (StatusCode, String)> {
    app.monitor
        .connections(index)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}
#[derive(serde::Deserialize)]
struct HistoryWindow {
    from: u64,
    to: u64,
}
async fn history_window(
    State(app): State<App>,
    Query(query): Query<HistoryWindow>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if query.from >= query.to || query.to - query.from > 21600 {
        return Err((
            StatusCode::BAD_REQUEST,
            "Choose a time window of at most six hours".into(),
        ));
    }
    app.db
        .history_window(&app.scope, app.demo, query.from, query.to)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e))
}
async fn history(State(app): State<App>) -> Result<Json<Value>, (StatusCode, String)> {
    app.db
        .history(&app.scope, app.demo)
        .await
        .map(Json)
        .map_err(db_error)
}
#[derive(serde::Deserialize)]
struct InventoryQuery {
    #[serde(default)]
    page: usize,
}
async fn monitor_inventory(
    State(app): State<App>,
    Path(kind): Path<String>,
    Query(query): Query<InventoryQuery>,
) -> Result<Json<Value>, (StatusCode, String)> {
    if query.page > 10000 || !["connections", "subscriptions"].contains(&kind.as_str()) {
        return Err((StatusCode::BAD_REQUEST, "Invalid inventory query".into()));
    }
    app.monitor
        .inventory(&kind, query.page)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_GATEWAY, e))
}
#[derive(serde::Deserialize)]
struct IncidentWindow {
    from: Option<u64>,
    to: Option<u64>,
}
async fn incident_history(
    State(app): State<App>,
    Query(query): Query<IncidentWindow>,
) -> Result<Json<Value>, (StatusCode, String)> {
    let window = match (query.from, query.to) {
        (None, None) => None,
        (Some(from), Some(to)) if from < to && to - from <= 21600 => Some((from, to)),
        _ => {
            return Err((
                StatusCode::BAD_REQUEST,
                "Invalid incident time window".into(),
            ));
        }
    };
    app.db
        .incident_history(&app.scope, app.demo, window)
        .await
        .map(Json)
        .map_err(db_error)
}
async fn activity(State(app): State<App>) -> Result<Json<Value>, (StatusCode, String)> {
    app.db
        .activity(&app.scope, app.demo)
        .await
        .map(Json)
        .map_err(db_error)
}
async fn settings(State(app): State<App>) -> Result<Json<store::Settings>, (StatusCode, String)> {
    app.db.settings().await.map(Json).map_err(db_error)
}
fn db_error(_: String) -> (StatusCode, String) {
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "Dashboard storage operation failed".into(),
    )
}
async fn save_settings(
    State(app): State<App>,
    headers: HeaderMap,
    Json(settings): Json<store::Settings>,
) -> Result<Json<store::Settings>, (StatusCode, String)> {
    if headers
        .get("x-natsui-request")
        .and_then(|v| v.to_str().ok())
        != Some("1")
    {
        return Err((StatusCode::FORBIDDEN, "Missing request header".into()));
    }
    settings
        .validate()
        .map_err(|s| (StatusCode::BAD_REQUEST, s))?;
    app.db
        .save_settings(settings.clone(), &app.scope, app.demo)
        .await
        .map_err(db_error)?;
    *app.settings_cache.write().await = settings.clone();
    Ok(Json(settings))
}

async fn collect(app: App) {
    let mut last_status = String::new();
    let started = std::time::Instant::now();
    let mut previous_nodes = json!({});
    loop {
        let snapshot = if app.demo {
            simulation::snapshot(&app.scope, started.elapsed().as_secs())
        } else {
            let mut connection_issue = None;
            if app.nats.read().await.is_none()
                && let Some(config) = &app.connection
            {
                match config.connect().await {
                    Ok(client) => *app.nats.write().await = Some(client),
                    Err(reason) => connection_issue = Some(reason),
                }
            }
            let client = app.nats.read().await.clone();
            match client {
                Some(client) => match tokio::time::timeout(
                    Duration::from_secs(20),
                    telemetry::observe(&client, &app.prefix, &app.scope),
                )
                .await
                {
                    Ok(s) => s,
                    Err(_) => {
                        Snapshot::unavailable("Collection exceeded its time budget", &app.scope)
                    }
                },
                None => Snapshot::unavailable(
                    connection_issue
                        .as_deref()
                        .unwrap_or("NATS connection unavailable"),
                    &app.scope,
                ),
            }
        };
        if snapshot.status != last_status {
            let _ = app
                .db
                .event(
                    &app.scope,
                    app.demo,
                    "collection",
                    &format!("Collection state: {}", snapshot.status),
                )
                .await;
            last_status = snapshot.status.clone();
        }
        let monitoring = app.monitor.current.read().await.clone();
        let settings = match app.db.settings().await {
            Ok(settings) => {
                *app.settings_cache.write().await = settings.clone();
                settings
            }
            Err(_) => app.settings_cache.read().await.clone(),
        };
        let previous = app.current.read().await.clone();
        let events = incidents::changes(
            &previous,
            &snapshot,
            &previous_nodes,
            &monitoring,
            settings.backlog_threshold,
        );
        if let Err(error) = app.db.incidents(&app.scope, app.demo, events).await {
            eprintln!("Incident write failed: {error}");
        }
        previous_nodes = monitoring;
        if let Err(error) = app
            .db
            .sample_resources(&snapshot, &app.monitor.current.read().await.clone())
            .await
        {
            eprintln!("History write failed: {error}");
        }
        *app.current.write().await = snapshot;
        let settings = app.settings_cache.read().await.clone();
        tokio::time::sleep(Duration::from_secs(settings.refresh_seconds)).await;
    }
}

#[cfg(test)]
mod http_tests {
    use super::*;
    use tower::ServiceExt;
    #[tokio::test]
    async fn local_boundary_rejects_rebinding_and_cross_origin() {
        for (host, origin, expected) in [
            ("127.0.0.1:4321", "http://127.0.0.1:4321", StatusCode::OK),
            ("localhost:4321", "http://localhost:4321", StatusCode::OK),
            (
                "attacker.example",
                "http://attacker.example",
                StatusCode::FORBIDDEN,
            ),
            (
                "127.0.0.1:4321",
                "https://attacker.example",
                StatusCode::FORBIDDEN,
            ),
        ] {
            let app = Router::new()
                .route("/", get(|| async { "ok" }))
                .layer(axum::middleware::from_fn(local_request));
            let request = axum::http::Request::builder()
                .uri("/")
                .header("host", host)
                .header("origin", origin)
                .body(axum::body::Body::empty())
                .unwrap();
            let response = app.oneshot(request).await.unwrap();
            assert_eq!(response.status(), expected);
            if expected == StatusCode::OK {
                assert!(response.headers().contains_key("content-security-policy"));
            }
        }
    }
}
