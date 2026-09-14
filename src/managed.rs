use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::{collections::BTreeMap, io::Read, sync::OnceLock, time::Duration};
type Error = (StatusCode, String);
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Node {
    id: String,
    url: String,
    ca_file: String,
    token_file: String,
}
struct Remote {
    url: reqwest::Url,
    client: reqwest::Client,
    token: String,
}
static REMOTES: OnceLock<BTreeMap<String, Remote>> = OnceLock::new();
fn file(path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("Managed endpoint file is empty or oversized".into());
    }
    Ok(bytes)
}
pub fn initialize() -> Result<(), Box<dyn std::error::Error>> {
    let mut remotes = BTreeMap::new();
    if let Ok(path) = std::env::var("NATSUI_MANAGED_CONFIG_FILE") {
        if std::env::var("NATSUI_ALLOW_MANAGED").as_deref() != Ok("1")
            || std::env::var_os("NATSUI_AUTH_TOKEN_FILE").is_none()
        {
            return Err("Managed deployment control requires NATSUI_ALLOW_MANAGED=1 and dashboard authentication".into());
        }
        let nodes: Vec<Node> = serde_json::from_slice(&file(&path, 65536)?)
            .map_err(|_| "Invalid managed endpoint configuration")?;
        if nodes.is_empty() || nodes.len() > 32 {
            return Err("Configure 1 to 32 managed nodes".into());
        }
        for node in nodes {
            if node.id.is_empty()
                || node.id.len() > 48
                || !node
                    .id
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                || remotes.contains_key(&node.id)
            {
                return Err("Invalid or duplicate managed node name".into());
            }
            let url = reqwest::Url::parse(&node.url).map_err(|_| "Invalid managed HTTPS URL")?;
            if url.scheme() != "https"
                || url.host_str().is_none()
                || url.path() != "/"
                || url.query().is_some()
                || url.fragment().is_some()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                return Err("Managed endpoints must be HTTPS origins".into());
            }
            let token = String::from_utf8(file(&node.token_file, 512)?)?
                .trim()
                .to_owned();
            if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
                return Err("Invalid managed controller token".into());
            }
            let mut builder = reqwest::Client::builder()
                .no_proxy()
                .redirect(reqwest::redirect::Policy::none())
                .timeout(Duration::from_secs(30));
            let roots = reqwest::Certificate::from_pem_bundle(&file(&node.ca_file, 1048576)?)?;
            if roots.is_empty() {
                return Err("Managed CA bundle has no certificates".into());
            }
            for root in roots {
                builder = builder.add_root_certificate(root);
            }
            remotes.insert(
                node.id,
                Remote {
                    url,
                    client: builder.build()?,
                    token,
                },
            );
        }
    }
    REMOTES
        .set(remotes)
        .map_err(|_| "Managed endpoints already initialized")?;
    Ok(())
}
pub async fn nodes() -> Json<Value> {
    Json(
        json!({"nodes":REMOTES.get().map(|r|r.keys().cloned().collect::<Vec<_>>()).unwrap_or_default(),"authority":"Optional controllers. Each owns one NATS process and its config-based users."}),
    )
}
async fn request(id: &str, path: &str, body: Option<Value>) -> Result<Value, Error> {
    let remote = REMOTES.get().and_then(|r| r.get(id)).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            "Managed node is not configured".into(),
        )
    })?;
    let url = remote
        .url
        .join(&format!("v1/{path}"))
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid controller path".into()))?;
    let request = if let Some(body) = body {
        remote.client.post(url).json(&body)
    } else {
        remote.client.get(url)
    };
    let mut response = request
        .bearer_auth(&remote.token)
        .send()
        .await
        .map_err(|_| {
            (
                StatusCode::BAD_GATEWAY,
                "Managed controller unavailable. Inspect state before retrying.".into(),
            )
        })?;
    let status = response.status();
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Managed response interrupted".into(),
        )
    })? {
        if bytes.len() + chunk.len() > 1048576 {
            return Err((
                StatusCode::BAD_GATEWAY,
                "Managed response exceeds 1 MiB".into(),
            ));
        }
        bytes.extend(chunk);
    }
    if !status.is_success() {
        return Err((
            StatusCode::BAD_GATEWAY,
            format!(
                "Controller rejected the operation (HTTP {status}). Reload and review before retrying."
            ),
        ));
    }
    serde_json::from_slice(&bytes).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Invalid controller response".into(),
        )
    })
}
pub async fn status(Path(id): Path<String>) -> Result<Json<Value>, Error> {
    request(&id, "status", None).await.map(Json)
}
pub async fn preview(
    State(app): State<crate::App>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<Value>, Error> {
    if app.demo {
        return Err((
            StatusCode::FORBIDDEN,
            "Managed writes are unavailable in simulation".into(),
        ));
    }
    let fields = body.as_object_mut().ok_or((
        StatusCode::BAD_REQUEST,
        "Expected an operation object".into(),
    ))?;
    fields.insert("owner".into(), app.auth.review_owner(&headers).into());
    request(&id, "preview", Some(body)).await.map(Json)
}
pub async fn apply(
    State(app): State<crate::App>,
    Path(id): Path<String>,
    headers: HeaderMap,
    Json(mut body): Json<Value>,
) -> Result<Json<Value>, Error> {
    if app.demo {
        return Err((
            StatusCode::FORBIDDEN,
            "Managed writes are unavailable in simulation".into(),
        ));
    }
    let fields = body.as_object_mut().ok_or((
        StatusCode::BAD_REQUEST,
        "Expected an operation object".into(),
    ))?;
    fields.insert("owner".into(), app.auth.review_owner(&headers).into());
    app.db
        .event(
            &app.scope,
            false,
            "managed_operation_requested",
            &format!(
                "actor={}, node={id}. Reviewed operation requested.",
                app.auth.actor(&headers)
            ),
        )
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Audit unavailable. No controller operation was sent.".into(),
            )
        })?;
    let result = request(&id, "apply", Some(body)).await;
    let saved = app
        .db
        .event(
            &app.scope,
            false,
            if result.is_ok() {
                "managed_operation_result"
            } else {
                "managed_operation_uncertain"
            },
            &format!(
                "actor={}, node={id}. Inspect controller status for verification.",
                app.auth.actor(&headers)
            ),
        )
        .await
        .is_ok();
    result.map(|mut value| {
        value["audit_saved"] = json!(saved);
        Json(value)
    })
}
