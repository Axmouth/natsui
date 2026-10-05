use crate::{App, telemetry};
use axum::{
    Extension, Json,
    extract::{Query, State},
    http::StatusCode,
};
use futures_util::StreamExt;
use serde::Deserialize;
use serde_json::{Value, json};
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::sync::{Mutex, Semaphore};

pub struct SessionTools {
    pub db: crate::store::Database,
    pub monitoring: AtomicBool,
    pub servers: Mutex<Vec<String>>,
    pub watch: Semaphore,
    pub certificate: Option<Value>,
}
impl SessionTools {
    pub fn new(certificate: Option<Value>) -> Result<Self, String> {
        Ok(Self {
            db: crate::store::Database::session().map_err(|_| "Session storage unavailable")?,
            monitoring: AtomicBool::new(false),
            servers: Mutex::new(vec![]),
            watch: Semaphore::new(1),
            certificate,
        })
    }
}
type Error = (StatusCode, String);
fn upstream() -> Error {
    (StatusCode::BAD_GATEWAY, "No usable NATS monitoring reply. Check system service access, reply permissions and connectivity. Cluster size is unknown.".into())
}

pub async fn session_info(
    Extension(session): Extension<Arc<crate::nats_login::Session>>,
) -> Json<Value> {
    Json(
        json!({"certificate":session.tools.certificate,"monitoring":session.tools.monitoring.load(Ordering::Relaxed)}),
    )
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MonitoringChoice {
    enabled: bool,
}
pub async fn set_monitoring(
    State(app): State<App>,
    Extension(session): Extension<Arc<crate::nats_login::Session>>,
    Json(choice): Json<MonitoringChoice>,
) -> Result<Json<Value>, Error> {
    if !app.auth.nats_policy().session_only {
        return Err((
            StatusCode::FORBIDDEN,
            "Session monitoring requires session-only mode".into(),
        ));
    }
    session
        .tools
        .monitoring
        .store(choice.enabled, Ordering::Relaxed);
    Ok(Json(json!({"enabled":choice.enabled})))
}

// One bounded request gathers replies from the servers that the NATS identity can reach.
// Missing replies never establish complete cluster coverage or an empty inventory.
async fn replies(
    client: &async_nats::Client,
    subject: String,
    body: Value,
) -> Result<Vec<Value>, Error> {
    let inbox = client.new_inbox();
    let mut sub = client
        .subscribe(inbox.clone())
        .await
        .map_err(|_| upstream())?;
    sub.unsubscribe_after(33).await.map_err(|_| upstream())?;
    client
        .publish_with_reply(subject, inbox, body.to_string().into())
        .await
        .map_err(|_| upstream())?;
    let deadline = tokio::time::Instant::now() + Duration::from_millis(1200);
    let mut result = vec![];
    let mut bytes = 0;
    while let Ok(Some(message)) = tokio::time::timeout_at(deadline, sub.next()).await {
        bytes += message.payload.len();
        if result.len() >= 32 || bytes > 4_000_000 || message.payload.len() > 2_000_000 {
            break;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&message.payload)
            && value["error"].is_null()
            && value["data"].is_object()
            && value["server"]["id"]
                .as_str()
                .is_some_and(telemetry::valid_token)
        {
            result.push(value);
        }
    }
    if result.is_empty() {
        Err(upstream())
    } else {
        Ok(result)
    }
}
pub async fn observe(client: &async_nats::Client, tools: &SessionTools) -> Value {
    let Ok(values) = replies(client, "$SYS.REQ.SERVER.PING.VARZ".into(), json!({})).await else {
        return json!({"status":"unavailable","source":"session_nats","nodes":[],"detail":upstream().1});
    };
    let mut slots = tools.servers.lock().await;
    let mut nodes = vec![];
    for value in values {
        let id = value["server"]["id"].as_str().unwrap();
        if !slots.iter().any(|v| v == id) && slots.len() < 32 {
            slots.push(id.into());
        }
        if let Some(slot) = slots.iter().position(|v| v == id) {
            if nodes.iter().any(|v: &Value| v["slot"] == slot) {
                continue;
            }
            nodes.push(crate::monitoring::project_node(
                &value["data"],
                slot,
                telemetry::now(),
            ));
        }
    }
    json!({"status":"partial","source":"session_nats","nodes":nodes,"detail":"NATS responders only. Total cluster size and silent servers are unknown. Up to 32 replies and 4 MB per request. This session tracks at most 32 distinct server identities."})
}
pub async fn inventory(
    client: &async_nats::Client,
    tools: &SessionTools,
    kind: &str,
    page: usize,
    server: Option<&str>,
) -> Result<Value, Error> {
    if page > 10000 {
        return Err((StatusCode::BAD_REQUEST, "Invalid page".into()));
    }
    let endpoint = match kind {
        "connections" => "CONNZ",
        "subscriptions" => "SUBSZ",
        "routes" => "ROUTEZ",
        _ => return Err((StatusCode::BAD_REQUEST, "Unknown inventory".into())),
    };
    let target = server.unwrap_or("PING");
    if !telemetry::valid_token(target) {
        return Err((StatusCode::BAD_REQUEST, "Invalid server identity".into()));
    }
    let values = replies(
        client,
        format!("$SYS.REQ.SERVER.{target}.{endpoint}"),
        json!({"offset":page*100,"limit":100,"subscriptions":kind!="routes"}),
    )
    .await?;
    let mut nodes = vec![];
    let mut slots = tools.servers.lock().await;
    for value in values {
        let id = value["server"]["id"].as_str().unwrap();
        if server.is_some_and(|expected| expected != id) {
            continue;
        }
        if !slots.iter().any(|v| v == id) && slots.len() < 32 {
            slots.push(id.into());
        }
        let Some(slot) = slots.iter().position(|v| v == id) else {
            continue;
        };
        if nodes.iter().any(|v: &Value| v["slot"] == slot) {
            continue;
        }
        let mut raw = value["data"].clone();
        raw["server_id"] = value["server"]["id"].clone();
        raw["server_name"] = value["server"]["name"].clone();
        let projected = crate::monitoring::project_inventory(kind, &raw).map_err(|_| upstream())?;
        nodes.push(json!({"slot":slot,"server_id":raw["server_id"],"name":raw["server_name"],"at":telemetry::now(),"status":if projected["truncated"]==true {"partial"}else{"complete"},"rows":projected["rows"],"total":projected["total"]}));
    }
    if nodes.is_empty() {
        return Err(upstream());
    }
    if server.is_some() {
        let node = &nodes[0];
        return Ok(
            json!({"connections":node["rows"],"total":node["total"],"limit":100,"at":telemetry::now()}),
        );
    }
    Ok(
        json!({"kind":kind,"page":page,"limit":100,"nodes":nodes,"at":telemetry::now(),"scope":"NATS session permissions. Responding servers only, total cluster coverage unknown."}),
    )
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WatchQuery {
    stream: String,
}
pub async fn watch(
    State(app): State<App>,
    session: Option<Extension<Arc<crate::nats_login::Session>>>,
    Query(query): Query<WatchQuery>,
) -> Result<Json<Value>, Error> {
    if app.demo || !telemetry::valid_token(&query.stream) {
        return Err((StatusCode::BAD_REQUEST, "Select a live stream".into()));
    }
    static CAPACITY: Semaphore = Semaphore::const_new(16);
    let _capacity = CAPACITY.try_acquire().map_err(|_| {
        (
            StatusCode::TOO_MANY_REQUESTS,
            "Failure watch capacity reached".into(),
        )
    })?;
    let _session_permit = match &session {
        Some(Extension(session)) => Some(session.tools.watch.try_acquire().map_err(|_| {
            (
                StatusCode::CONFLICT,
                "A watch is already running for this session".into(),
            )
        })?),
        None => None,
    };
    let client = app.nats.read().await.clone().ok_or_else(upstream)?;
    let mut subscriptions = futures_util::stream::SelectAll::new();
    for kind in ["MAX_DELIVERIES", "MSG_TERMINATED"] {
        let mut sub = client
            .subscribe(format!(
                "$JS.EVENT.ADVISORY.CONSUMER.{kind}.{}.*",
                query.stream
            ))
            .await
            .map_err(|_| upstream())?;
        sub.unsubscribe_after(121).await.map_err(|_| upstream())?;
        subscriptions.push(sub);
    }
    tokio::time::timeout(Duration::from_secs(2), client.flush())
        .await
        .map_err(|_| upstream())?
        .map_err(|_| upstream())?;
    let started = telemetry::now();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(8);
    let mut events = vec![];
    let mut received = 0;
    let mut received_bytes = 0;
    let mut gap = false;
    let mut end_reason = "window elapsed";
    let domain = app
        .prefix
        .strip_prefix("$JS.")
        .and_then(|s| s.strip_suffix(".API"));
    let mut connection_check = tokio::time::interval(Duration::from_millis(250));
    loop {
        let message = tokio::select! {
            _ = tokio::time::sleep_until(deadline) => break,
            _ = connection_check.tick() => {
                if client.connection_state() != async_nats::connection::State::Connected {
                    gap = true;
                    end_reason = "connection lost before the deadline";
                    break;
                }
                continue;
            },
            message = subscriptions.next() => match message {
                Some(message) => message,
                None => { gap = true; end_reason = "subscription closed before the deadline"; break; },
            },
        };
        received += 1;
        received_bytes += message.payload.len();
        if received > 120 || received_bytes > 1_000_000 || message.payload.len() > 65536 {
            gap = true;
            end_reason = "recording limit reached";
            break;
        }
        if let Ok(value) = serde_json::from_slice::<Value>(&message.payload)
            && value["stream"] == query.stream
            && domain.is_none_or(|domain| value["domain"].as_str() == Some(domain))
            && matches!(
                value["type"].as_str(),
                Some(
                    "io.nats.jetstream.advisory.v1.max_deliver"
                        | "io.nats.jetstream.advisory.v1.terminated"
                )
            )
            && let Some(event) = crate::incidents::advisory(&value)
        {
            events.push(event);
        }
    }
    drop(subscriptions);
    let detail = "Eight-second watch window. Silence does not confirm subscription permission or absence of failures. Events outside windows cannot be recovered. Application publishers may also publish advisories.";
    let mut recorded = events.clone();
    recorded.push(json!({"at":telemetry::now(),"kind":"advisory-coverage","resource":"stream","stream":query.stream,"name":query.stream,"source":"session watch","detail":format!("{end_reason}. {detail}"),"started":started,"gap":gap,"end_reason":end_reason}));
    app.db
        .incidents(&app.scope, false, recorded)
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Failure event storage unavailable".into(),
            )
        })?;
    Ok(Json(
        json!({"events":events,"started":started,"ended":telemetry::now(),"gap":gap,"end_reason":end_reason,"detail":detail}),
    ))
}
