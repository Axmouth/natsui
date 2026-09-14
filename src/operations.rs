use crate::{App, messages, telemetry};
use axum::{
    Json,
    extract::State,
    http::{HeaderMap, StatusCode},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::time::Duration;

type Error = (StatusCode, String);
fn bad(message: &str) -> Error {
    (StatusCode::BAD_REQUEST, message.into())
}
fn conflict(message: &str) -> Error {
    (StatusCode::CONFLICT, message.into())
}
fn upstream(_: impl std::fmt::Display) -> Error {
    (
        StatusCode::BAD_GATEWAY,
        "NATS operation failed or timed out. Inspect current state before retrying.".into(),
    )
}
#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    action: String,
    #[serde(default)]
    stream: String,
    #[serde(default)]
    consumer: String,
    #[serde(default)]
    config: Value,
    #[serde(default)]
    subject: String,
    #[serde(default)]
    payload: String,
    #[serde(default)]
    mode: String,
}
pub struct Preview {
    proposal: Proposal,
    revision: Option<String>,
    confirmation: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    token: String,
    confirmation: String,
}
fn writable(app: &App, headers: &HeaderMap) -> Result<(), Error> {
    if app.demo
        || !app.editor.allowed
        || headers
            .get("x-natsui-request")
            .and_then(|v| v.to_str().ok())
            != Some("1")
        || app.auth.role(headers) == Some(crate::auth::Role::Viewer)
    {
        return Err((
            StatusCode::FORBIDDEN,
            "NATS writes are disabled for this session or deployment.".into(),
        ));
    }
    Ok(())
}
fn name(value: &str) -> Result<(), Error> {
    if !telemetry::valid_token(value) || value.len() > 128 {
        return Err(bad("Invalid resource name"));
    }
    Ok(())
}
fn normalize(p: &mut Proposal) -> Result<String, Error> {
    if p.action == "publish" {
        if !messages::valid_filter(&p.subject)
            || p.subject.contains(['*', '>'])
            || p.subject.starts_with('$')
            || p.payload.len() > 65536
            || !matches!(p.mode.as_str(), "core" | "jetstream")
        {
            return Err(bad(
                "Use a literal application subject and a payload of at most 64 KiB. Choose Core or JetStream explicitly.",
            ));
        }
        if p.mode == "jetstream" {
            name(&p.stream)?;
        }
        return Ok(p.subject.clone());
    }
    name(&p.stream)?;
    if p.stream.starts_with("KV_") || p.stream.starts_with("OBJ_") {
        return Err(bad(
            "Use dedicated bucket operations for KV and Object Store resources.",
        ));
    }
    if p.action.ends_with("consumer") {
        name(&p.consumer)?;
    }
    match p.action.as_str() {
        "create_stream" => {
            let config = p
                .config
                .as_object()
                .ok_or_else(|| bad("Stream configuration is required"))?;
            if config.keys().any(|k| {
                ![
                    "subjects",
                    "storage",
                    "retention",
                    "num_replicas",
                    "max_msgs",
                    "max_bytes",
                    "max_age",
                ]
                .contains(&k.as_str())
            }) {
                return Err(bad("Unsupported stream creation setting"));
            }
            let subjects = config
                .get("subjects")
                .and_then(Value::as_array)
                .ok_or_else(|| bad("Captured subjects are required"))?;
            if subjects.is_empty()
                || subjects.len() > 100
                || subjects
                    .iter()
                    .any(|s| !s.as_str().is_some_and(messages::valid_filter))
            {
                return Err(bad("Configure 1 to 100 valid captured subjects"));
            }
            let storage = config
                .get("storage")
                .and_then(Value::as_str)
                .unwrap_or("file");
            let retention = config
                .get("retention")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| bad("retention must be a string"))
                })
                .transpose()?
                .unwrap_or("limits");
            let replicas = config
                .get("num_replicas")
                .map(|value| {
                    value
                        .as_u64()
                        .ok_or_else(|| bad("Replica count must be an integer"))
                })
                .transpose()?
                .unwrap_or(1);
            if !["file", "memory"].contains(&storage)
                || !["limits", "workqueue", "interest"].contains(&retention)
                || !(1..=5).contains(&replicas)
            {
                return Err(bad("Invalid storage, retention or replica setting"));
            }
            let mut normalized = json!({"name":p.stream,"subjects":subjects,"storage":storage,"retention":retention,"num_replicas":replicas});
            for field in ["max_msgs", "max_bytes", "max_age"] {
                if let Some(value) = config.get(field) {
                    let number = value
                        .as_i64()
                        .ok_or_else(|| bad("Limits must be exact integers"))?;
                    if (field == "max_age" && number < 0)
                        || (field != "max_age" && number != -1 && number <= 0)
                        || number > 9_007_199_254_740_991
                    {
                        return Err(bad("Invalid resource limit"));
                    }
                    normalized[field] = value.clone();
                }
            }
            p.config = normalized;
        }
        "create_consumer" => {
            let config = p
                .config
                .as_object()
                .ok_or_else(|| bad("Consumer configuration is required"))?;
            if config.keys().any(|k| {
                ![
                    "filter_subject",
                    "deliver_policy",
                    "ack_wait",
                    "max_ack_pending",
                    "max_deliver",
                ]
                .contains(&k.as_str())
            }) {
                return Err(bad("Unsupported consumer creation setting"));
            }
            let filter = config
                .get("filter_subject")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| bad("filter_subject must be a string"))
                })
                .transpose()?
                .unwrap_or("");
            let policy = config
                .get("deliver_policy")
                .map(|value| {
                    value
                        .as_str()
                        .ok_or_else(|| bad("deliver_policy must be a string"))
                })
                .transpose()?
                .unwrap_or("all");
            if (!filter.is_empty() && !messages::valid_filter(filter))
                || !["all", "new"].contains(&policy)
            {
                return Err(bad("Invalid consumer filter or delivery policy"));
            }
            let mut normalized = json!({"durable_name":p.consumer,"name":p.consumer,"ack_policy":"explicit","deliver_policy":policy,"filter_subject":filter});
            for field in ["ack_wait", "max_ack_pending", "max_deliver"] {
                if let Some(value) = config.get(field) {
                    let number = value
                        .as_i64()
                        .ok_or_else(|| bad("Consumer limits must be exact integers"))?;
                    if (field == "ack_wait" && number <= 0)
                        || (field != "ack_wait" && number != -1 && number <= 0)
                        || number > 9_007_199_254_740_991
                    {
                        return Err(bad("Invalid consumer limit"));
                    }
                    normalized[field] = value.clone();
                }
            }
            p.config = normalized;
        }
        "delete_stream" | "delete_consumer" => {}
        _ => return Err(bad("Unsupported operation")),
    }
    Ok(if p.action.ends_with("consumer") {
        p.consumer.clone()
    } else {
        p.stream.clone()
    })
}
async fn lookup(
    client: &async_nats::Client,
    prefix: &str,
    p: &Proposal,
) -> Result<Option<Value>, Error> {
    let suffix = if p.action.ends_with("consumer") {
        format!("CONSUMER.INFO.{}.{}", p.stream, p.consumer)
    } else {
        format!("STREAM.INFO.{}", p.stream)
    };
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        client.request(format!("{prefix}.{suffix}"), "{}".into()),
    )
    .await
    .map_err(upstream)?
    .map_err(upstream)?;
    if response.payload.len() > 2_000_000 {
        return Err(upstream("oversized"));
    }
    let value: Value = serde_json::from_slice(&response.payload).map_err(upstream)?;
    if value["error"]["err_code"] == 10059 || value["error"]["err_code"] == 10014 {
        return Ok(None);
    }
    if value.get("error").is_some() || !value["created"].is_string() || !value["config"].is_object()
    {
        return Err(upstream("invalid"));
    }
    Ok(Some(value))
}
fn revision(value: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(json!([value["created"], value["config"]]).to_string())
    )
}
async fn client(app: &App) -> Result<async_nats::Client, Error> {
    app.nats.read().await.clone().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "NATS is disconnected".into(),
        )
    })
}
pub async fn preview(
    State(app): State<App>,
    headers: HeaderMap,
    Json(mut proposal): Json<Proposal>,
) -> Result<Json<Value>, Error> {
    writable(&app, &headers)?;
    let confirmation = normalize(&mut proposal)?;
    let client = client(&app).await?;
    if !crate::editing::supported(&client.server_info().version) {
        return Err(bad("Native operations require NATS 2.11 or newer"));
    }
    let observed = if proposal.action == "publish" {
        None
    } else {
        lookup(&client, &app.prefix, &proposal).await?
    };
    if proposal.action.starts_with("create") && observed.is_some() {
        return Err(conflict(
            "Resource already exists. No creation request was sent.",
        ));
    }
    if proposal.action.starts_with("delete") && observed.is_none() {
        return Err(conflict("Resource no longer exists."));
    }
    let mut result = json!({"expires_in":60,"confirmation":confirmation,"proposal":proposal,"current":observed,"warning":if proposal.action=="delete_stream"{"Deletion removes retained records and all consumers. NATS has no atomic revision-checked deletion. External replacement between revalidation and deletion cannot be excluded."}else if proposal.action=="delete_consumer"{"Deletion removes the durable cursor and delivery state. It does not stop application processes. NATS has no atomic revision-checked deletion."}else if proposal.action=="publish"{"Core acceptance does not prove storage or processing. JetStream acknowledgment proves storage in the named stream, not application processing. A timeout has an uncertain outcome. No automatic retry is sent."}else{"Creation applies live. Retention and capture rules affect storage and delivery. No application worker is started."}});
    let token = app.editor.operation_pending.insert(
        app.auth.review_owner(&headers),
        Preview {
            proposal,
            revision: observed.as_ref().map(revision),
            confirmation,
        },
        Duration::from_secs(60),
    )?;
    result["token"] = token.into();
    Ok(Json(result))
}
pub async fn apply(
    State(app): State<App>,
    headers: HeaderMap,
    Json(approval): Json<Approval>,
) -> Result<Json<Value>, Error> {
    writable(&app, &headers)?;
    let _mutation = app.editor.mutation.lock().await;
    let preview = app.editor.operation_pending.take(
        &approval.token,
        &app.auth.review_owner(&headers),
        |preview| preview.confirmation == approval.confirmation,
    )?;
    let p = preview.proposal;
    let client = client(&app).await?;
    if !crate::editing::supported(&client.server_info().version) {
        return Err(bad("Connected server no longer supports native operations"));
    }
    if p.action != "publish" {
        let current = lookup(&client, &app.prefix, &p).await?;
        if current.as_ref().map(revision) != preview.revision {
            return Err(conflict(
                "Resource changed since review. No operation was sent.",
            ));
        }
    }
    let detail = format!(
        "actor={}, operation={}, {}: stream={}, consumer={}, subject={}",
        app.auth.actor(&headers),
        approval.token,
        p.action,
        p.stream,
        p.consumer,
        p.subject
    );
    app.db
        .event(&app.scope, false, "operation_requested", &detail)
        .await
        .map_err(|_| {
            (
                StatusCode::SERVICE_UNAVAILABLE,
                "Audit storage unavailable. No operation was sent.".into(),
            )
        })?;
    let result = if p.action == "publish" {
        if p.mode == "core" {
            tokio::time::timeout(Duration::from_secs(5),async {client.publish(p.subject.clone(),p.payload.clone().into()).await.map_err(upstream)?;client.flush().await.map_err(upstream)?;Ok::<Value,Error>(json!({"mode":"core","accepted_by_server":true,"storage_verified":false,"processing_verified":false}))}).await.map_err(upstream).and_then(|r|r)
        } else {
            let mut headers = async_nats::HeaderMap::new();
            headers.insert("Nats-Expected-Stream", p.stream.as_str());
            let js = async_nats::jetstream::with_prefix(client.clone(), &app.prefix);
            tokio::time::timeout(Duration::from_secs(5),async {let ack=js.publish_with_headers(p.subject.clone(),headers,p.payload.clone().into()).await.map_err(upstream)?.await.map_err(upstream)?;Ok::<Value,Error>(json!({"mode":"jetstream","stream":ack.stream,"sequence":ack.sequence,"duplicate":ack.duplicate,"storage_verified":true,"processing_verified":false}))}).await.map_err(upstream).and_then(|r|r)
        }
    } else {
        let (suffix, body) = match p.action.as_str() {
            "create_stream" => (format!("STREAM.CREATE.{}", p.stream), p.config.clone()),
            "create_consumer" => (
                format!("CONSUMER.CREATE.{}.{}", p.stream, p.consumer),
                json!({"stream_name":p.stream,"config":p.config,"action":"create"}),
            ),
            "delete_stream" => (format!("STREAM.DELETE.{}", p.stream), json!({})),
            "delete_consumer" => (
                format!("CONSUMER.DELETE.{}.{}", p.stream, p.consumer),
                json!({}),
            ),
            _ => return Err(bad("Unsupported operation")),
        };
        match telemetry::request(&client, format!("{}.{suffix}", app.prefix), body).await {
            Ok(_) => {
                let observed = lookup(&client, &app.prefix, &p).await;
                let verified = observed.as_ref().is_ok_and(|state| {
                    if p.action.starts_with("delete") {
                        state.is_none()
                    } else {
                        state.is_some()
                    }
                });
                Ok(json!({"accepted":true,"verified":verified,"resource":preview.confirmation}))
            }
            Err(e) => Err(upstream(e)),
        }
    };
    let audit_saved = app
        .db
        .event(
            &app.scope,
            false,
            if result.is_ok() {
                "operation_result"
            } else {
                "operation_uncertain"
            },
            &detail,
        )
        .await
        .is_ok();
    match result {
        Ok(mut value) => {
            value["audit_saved"] = json!(audit_saved);
            Ok(Json(value))
        }
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn proposal(value: Value) -> Proposal {
        serde_json::from_value(value).unwrap()
    }
    #[test]
    fn lifecycle_validation_preserves_native_semantics() {
        for value in [
            json!({"action":"publish","mode":"core","subject":"orders.*"}),
            json!({"action":"publish","mode":"core","subject":"$JS.API.STREAM.DELETE.X"}),
            json!({"action":"delete_stream","stream":"KV_secrets"}),
            json!({"action":"create_consumer","stream":"ORDERS","consumer":"worker","config":{"deliver_subject":"production.worker"}}),
            json!({"action":"create_stream","stream":"ORDERS","config":{"subjects":["orders.>"],"num_replicas":6}}),
        ] {
            assert!(normalize(&mut proposal(value)).is_err());
        }
        let mut consumer = proposal(
            json!({"action":"create_consumer","stream":"ORDERS","consumer":"worker","config":{"filter_subject":"orders.*","deliver_policy":"new"}}),
        );
        normalize(&mut consumer).unwrap();
        assert_eq!(consumer.config["ack_policy"], "explicit");
        assert!(consumer.config.get("deliver_subject").is_none());
    }
}
