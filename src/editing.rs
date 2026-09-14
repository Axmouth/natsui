use crate::{App, telemetry};
use axum::{
    Json,
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Arc, time::Duration};
use tokio::sync::Mutex;

type Error = (StatusCode, String);
#[derive(Clone, Default)]
pub struct Editor {
    pub allowed: bool,
    pub mutation: Arc<Mutex<()>>,
    pub operation_pending: Arc<crate::reviews::Reviews<crate::operations::Preview>>,
    pending: Arc<crate::reviews::Reviews<Preview>>,
}
impl Editor {
    pub fn new(allowed: bool) -> Self {
        Self {
            allowed,
            ..Self::default()
        }
    }
}
struct Preview {
    target: Target,
    revision: String,
    config: Value,
    changes: Value,
    warnings: Vec<String>,
}
#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    kind: String,
    stream: String,
    #[serde(default)]
    consumer: String,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    target: Target,
    revision: String,
    changes: BTreeMap<String, String>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    token: String,
    accept_warnings: bool,
}
fn error(code: StatusCode, text: &str) -> Error {
    (code, text.into())
}
fn bad(text: &str) -> Error {
    error(StatusCode::BAD_REQUEST, text)
}
fn header(headers: &HeaderMap) -> Result<(), Error> {
    if headers
        .get("x-natsui-request")
        .and_then(|v| v.to_str().ok())
        != Some("1")
    {
        return Err(error(StatusCode::FORBIDDEN, "Missing request header"));
    }
    Ok(())
}
fn writable(app: &App, headers: &HeaderMap) -> Result<(), Error> {
    header(headers)?;
    if !app.editor.allowed || app.demo {
        return Err(error(
            StatusCode::FORBIDDEN,
            "Broker editing is disabled. Set NATSUI_ALLOW_WRITES=1 for this local profile and restart the dashboard.",
        ));
    }
    Ok(())
}
impl Target {
    fn validate(&self) -> Result<(), Error> {
        if !telemetry::valid_token(&self.stream)
            || !matches!(self.kind.as_str(), "stream" | "consumer")
            || (self.kind == "consumer" && !telemetry::valid_token(&self.consumer))
            || (self.kind == "stream" && !self.consumer.is_empty())
        {
            return Err(bad("Invalid resource target"));
        }
        Ok(())
    }
    fn info_subject(&self, prefix: &str) -> String {
        if self.kind == "stream" {
            format!("{prefix}.STREAM.INFO.{}", self.stream)
        } else {
            format!("{prefix}.CONSUMER.INFO.{}.{}", self.stream, self.consumer)
        }
    }
    fn update(&self, prefix: &str, config: Value) -> (String, Value) {
        if self.kind == "stream" {
            (format!("{prefix}.STREAM.UPDATE.{}", self.stream), config)
        } else {
            (
                format!("{prefix}.CONSUMER.CREATE.{}.{}", self.stream, self.consumer),
                json!({"stream_name":self.stream,"config":config,"action":"update"}),
            )
        }
    }
    fn name(&self) -> &str {
        if self.kind == "stream" {
            &self.stream
        } else {
            &self.consumer
        }
    }
}
async fn client(app: &App) -> Result<async_nats::Client, Error> {
    if app.demo {
        return Err(bad("Simulated resources cannot be edited"));
    }
    app.nats
        .read()
        .await
        .clone()
        .ok_or_else(|| error(StatusCode::SERVICE_UNAVAILABLE, "NATS is disconnected"))
}
async fn info(client: &async_nats::Client, prefix: &str, target: &Target) -> Result<Value, Error> {
    let v = telemetry::request(client, target.info_subject(prefix), json!({}))
        .await
        .map_err(|e| error(StatusCode::BAD_GATEWAY, &e))?;
    if !v["config"].is_object() || !v["created"].is_string() {
        return Err(error(
            StatusCode::BAD_GATEWAY,
            "Incomplete resource configuration",
        ));
    }
    Ok(v)
}
fn revision(v: &Value) -> String {
    format!(
        "{:x}",
        Sha256::digest(json!([v["created"], v["config"]]).to_string())
    )
}
// The editor targets 2.11+ so update-only consumer requests cannot fall back to creation.
pub(crate) fn supported(version: &str) -> bool {
    let mut v = version
        .trim_start_matches('v')
        .split('.')
        .filter_map(|x| x.parse::<u32>().ok());
    matches!((v.next(),v.next()), (Some(2),Some(minor)) if minor >= 11)
}
fn keys(kind: &str) -> &'static [&'static str] {
    if kind == "stream" {
        &[
            "max_msgs",
            "max_bytes",
            "max_age",
            "max_msg_size",
            "subjects",
            "discard",
            "duplicate_window",
            "num_replicas",
        ]
    } else {
        &["ack_wait", "max_ack_pending", "max_deliver", "backoff"]
    }
}
fn display(key: &str, v: &Value) -> String {
    if v.is_null() {
        return if matches!(key, "subjects" | "backoff") {
            String::new()
        } else {
            "0".into()
        };
    }
    if matches!(key, "max_age" | "ack_wait" | "duplicate_window") {
        return seconds(v.as_i64().unwrap_or(0));
    }
    if let Some(a) = v.as_array() {
        return a
            .iter()
            .map(|v| {
                if key == "backoff" {
                    seconds(v.as_i64().unwrap_or(0))
                } else {
                    v.as_str().unwrap_or("").to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
    }
    v.as_str()
        .map(str::to_owned)
        .unwrap_or_else(|| v.to_string())
}
fn seconds(ns: i64) -> String {
    let full = format!("{}.{:09}", ns / 1_000_000_000, ns % 1_000_000_000);
    full.trim_end_matches('0').trim_end_matches('.').to_owned()
}
fn duration(s: &str) -> Result<i64, Error> {
    let (whole, frac) = s.split_once('.').unwrap_or((s, ""));
    if whole.is_empty()
        || !whole.bytes().all(|c| c.is_ascii_digit())
        || frac.len() > 9
        || !frac.bytes().all(|c| c.is_ascii_digit())
    {
        return Err(bad(
            "Duration must be nonnegative seconds with at most 9 decimal places",
        ));
    }
    let whole = whole
        .parse::<i64>()
        .map_err(|_| bad("Duration is too large"))?;
    whole
        .checked_mul(1_000_000_000)
        .and_then(|n| n.checked_add(format!("{frac:0<9}").parse().ok()?))
        .ok_or_else(|| bad("Duration is too large"))
}
fn parse(key: &str, input: &str) -> Result<Value, Error> {
    let input = input.trim();
    if input.len() > 8192 {
        return Err(bad("Setting exceeds input limit"));
    }
    if matches!(key, "max_age" | "ack_wait" | "duplicate_window") {
        let n = duration(input)?;
        if key != "max_age" && n == 0 {
            return Err(bad(
                "Acknowledgment timeout and duplicate window must be positive",
            ));
        }
        return Ok(json!(n));
    }
    if key == "subjects" {
        let a: Vec<_> = input.split(',').map(str::trim).collect();
        if a.is_empty()
            || a.len() > 128
            || a.iter().any(|s| {
                s.is_empty()
                    || s.len() > 1024
                    || s.chars().any(|c| c.is_whitespace() || c.is_control())
                    || s.split('.').enumerate().any(|(i, t)| {
                        t.is_empty()
                            || (t.contains('>') && (t != ">" || i + 1 != s.split('.').count()))
                            || (t.contains('*') && t != "*")
                    })
            })
        {
            return Err(bad(
                "Enter comma-separated NATS subjects; > must be the final token",
            ));
        }
        return Ok(json!(a));
    }
    if key == "backoff" {
        let a: Vec<i64> = if input.is_empty() {
            vec![]
        } else {
            input
                .split(',')
                .map(|s| duration(s.trim()))
                .collect::<Result<_, _>>()?
        };
        if a.len() > 32 || a.contains(&0) {
            return Err(bad("Backoff accepts at most 32 positive delays in seconds"));
        }
        return Ok(json!(a));
    }
    if key == "discard" {
        return if matches!(input, "old" | "new") {
            Ok(json!(input))
        } else {
            Err(bad("Discard must be old or new"))
        };
    }
    let n = input
        .parse::<i64>()
        .map_err(|_| bad("Limit must be an integer"))?;
    if (key == "num_replicas" && !(1..=5).contains(&n))
        || (key != "num_replicas" && (n < -1 || n == 0))
        || (key == "max_msg_size" && n > i32::MAX as i64)
    {
        return Err(bad(
            "Limits must be positive or -1 for unlimited; replicas must be 1 to 5",
        ));
    }
    Ok(json!(n))
}
fn candidate(
    target: &Target,
    current: &Value,
    patch: &BTreeMap<String, String>,
) -> Result<(Value, Value, Vec<String>), Error> {
    if patch.is_empty() || patch.len() > 8 {
        return Err(bad("Choose at least one supported setting to change"));
    }
    let mut config = current.clone();
    let mut changes = json!({});
    let mut warnings = vec![];
    for (key, text) in patch {
        if !keys(&target.kind).contains(&key.as_str()) {
            return Err(bad("Setting is not editable in this build"));
        }
        let value = parse(key, text)?;
        if current[key] == value {
            continue;
        }
        if key == "subjects" && !current["mirror"].is_null() {
            return Err(bad("Mirror stream capture subjects cannot be edited"));
        }
        if key == "ack_wait"
            && config["backoff"].as_array().is_some_and(|a| !a.is_empty())
            && !patch.get("backoff").is_some_and(|s| s.trim().is_empty())
        {
            return Err(bad(
                "Backoff overrides acknowledgment timeout; edit backoff or clear it first",
            ));
        }
        if matches!(key.as_str(), "max_age" | "max_bytes" | "max_msgs")
            && value.as_i64().is_some_and(|n| {
                n > 0
                    && (current[key].as_i64().unwrap_or(0) <= 0
                        || n < current[key].as_i64().unwrap_or(0))
            })
        {
            warnings.push(format!("Reducing {key} can immediately remove retained messages. Restoring the setting cannot recover those messages."));
        }
        if key == "subjects" {
            warnings.push("Subject changes affect future capture. Existing retained records remain subject to retention.".into());
        }
        if key == "num_replicas" {
            warnings.push("Replica changes can move data and alter fault tolerance; sufficient cluster capacity is required.".into());
        }
        changes[key] = json!({"before":display(key,&current[key]),"after":display(key,&value)});
        config[key] = value;
    }
    if changes.as_object().unwrap().is_empty() {
        return Err(bad("No configuration changes"));
    }
    if target.kind == "consumer" {
        if current["ack_policy"] == "none" {
            return Err(bad(
                "Delivery tuning requires an acknowledgment-based consumer",
            ));
        }
        if let Some(backoff) = config["backoff"].as_array()
            && !backoff.is_empty()
        {
            if config["max_deliver"]
                .as_i64()
                .is_some_and(|n| n > 0 && (backoff.len() as i64) > n)
            {
                return Err(bad(
                    "Backoff cannot contain more delays than maximum delivery attempts",
                ));
            }
            config["ack_wait"] = backoff[0].clone();
            if config["ack_wait"] != current["ack_wait"] {
                changes["ack_wait"] = json!({"before":display("ack_wait",&current["ack_wait"]),"after":display("ack_wait",&config["ack_wait"])});
            }
            warnings.push("Backoff controls acknowledgment timeouts and overrides Ack wait; ordinary negative acknowledgments do not use this schedule.".into());
        }
        warnings.push("Delivery tuning affects running workers. More outstanding acknowledgments can increase worker pressure; maximum delivery attempts does not create a dead-letter queue.".into());
    }
    Ok((config, changes, warnings))
}
pub async fn capabilities(State(app): State<App>, headers: HeaderMap) -> Json<Value> {
    Json(
        json!({"enabled":app.editor.allowed && !app.demo && app.auth.role(&headers)!=Some(crate::auth::Role::Viewer),"demo":app.demo,"profile":app.scope}),
    )
}
pub async fn read(
    State(app): State<App>,
    Query(target): Query<Target>,
) -> Result<Json<Value>, Error> {
    target.validate()?;
    let client = client(&app).await?;
    let value = info(&client, &app.prefix, &target).await?;
    let version = client.server_info().version;
    let mut fields = json!({});
    for key in keys(&target.kind) {
        fields[*key] = json!(display(key, &value["config"][*key]));
    }
    Ok(Json(
        json!({"revision":revision(&value),"fields":fields,"supported":supported(&version),"version":version,"created":value["created"]}),
    ))
}
pub async fn preview(
    State(app): State<App>,
    headers: HeaderMap,
    Json(proposal): Json<Proposal>,
) -> Result<Json<Value>, Error> {
    writable(&app, &headers)?;
    proposal.target.validate()?;
    let client = client(&app).await?;
    if !supported(&client.server_info().version) {
        return Err(bad("Editing requires NATS 2.11 or later in the 2.x series"));
    }
    let current = info(&client, &app.prefix, &proposal.target).await?;
    if revision(&current) != proposal.revision {
        return Err(error(
            StatusCode::CONFLICT,
            "Configuration changed. Reload the resource and review again.",
        ));
    }
    let (config, changes, warnings) =
        candidate(&proposal.target, &current["config"], &proposal.changes)?;
    let mut result = json!({"changes":changes,"warnings":warnings,"expires_seconds":120});
    let token = app.editor.pending.insert(
        app.auth.review_owner(&headers),
        Preview {
            target: proposal.target,
            revision: proposal.revision,
            config,
            changes,
            warnings,
        },
        Duration::from_secs(120),
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
    let p = app.editor.pending.take(
        &approval.token,
        &app.auth.review_owner(&headers),
        |preview| preview.warnings.is_empty() || approval.accept_warnings,
    )?;
    let client = client(&app).await?;
    if !supported(&client.server_info().version) {
        return Err(bad(
            "Connected server version no longer supports this editor",
        ));
    }
    let current = info(&client, &app.prefix, &p.target).await?;
    if revision(&current) != p.revision {
        return Err(error(
            StatusCode::CONFLICT,
            "Configuration changed since preview. Reload and review again.",
        ));
    }
    let event = |outcome: &str| json!({"at":telemetry::now(),"kind":"configuration","resource":p.target.kind,"stream":p.target.stream,"name":p.target.name(),"source":app.auth.actor(&headers),"operation":approval.token,"changes":p.changes,"detail":format!("Natsui configuration edit: {outcome}")});
    app.db
        .incidents(&app.scope, false, vec![event("attempt recorded")])
        .await
        .map_err(|_| {
            error(
                StatusCode::SERVICE_UNAVAILABLE,
                "Audit storage unavailable; no update was sent",
            )
        })?;
    let (subject, body) = p.target.update(&app.prefix, p.config.clone());
    let result = telemetry::request(&client, subject, body).await;
    let observed = info(&client, &app.prefix, &p.target).await;
    let verified = observed.as_ref().is_ok_and(|v| {
        v["created"] == current["created"]
            && p.changes
                .as_object()
                .unwrap()
                .keys()
                .all(|k| v["config"][k] == p.config[k])
    });
    let outcome = if verified {
        "applied and verified"
    } else if result.is_ok() {
        "accepted; readback not confirmed"
    } else {
        "outcome uncertain or rejected; inspect current settings before retrying"
    };
    let audit_saved = app
        .db
        .incidents(&app.scope, false, vec![event(outcome)])
        .await
        .is_ok();
    Ok(Json(
        json!({"verified":verified,"outcome":outcome,"audit_saved":audit_saved}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn target(kind: &str) -> Target {
        Target {
            kind: kind.into(),
            stream: "ORDERS".into(),
            consumer: if kind == "consumer" {
                "worker".into()
            } else {
                String::new()
            },
        }
    }
    #[test]
    fn exact_values_and_allowlist() {
        assert_eq!(duration("1.000000001").unwrap(), 1_000_000_001);
        assert!(duration("9223372037").is_err());
        assert!(duration("1.0000000001").is_err());
        let current =
            json!({"max_msgs":-1,"metadata":{"managed":"preserved"},"future":9007199254740993u64});
        let patch = BTreeMap::from([("max_msgs".into(), "9007199254740993".into())]);
        let (cfg, _, warnings) = candidate(&target("stream"), &current, &patch).unwrap();
        assert_eq!(cfg["max_msgs"], 9007199254740993u64);
        assert_eq!(cfg["future"], current["future"]);
        assert_eq!(cfg["metadata"], current["metadata"]);
        assert!(!warnings.is_empty());
        assert!(
            candidate(
                &target("stream"),
                &current,
                &BTreeMap::from([("storage".into(), "memory".into())])
            )
            .is_err()
        );
        assert!(parse("subjects", "foo.>.bar").is_err());
        assert!(parse("max_msgs", "0").is_err());
    }
    #[test]
    fn backoff_and_revision() {
        let current = json!({"ack_policy":"explicit","ack_wait":30_000_000_000i64,"max_deliver":5});
        let patch = BTreeMap::from([("backoff".into(), "1, 5".into())]);
        let (cfg, changes, _) = candidate(&target("consumer"), &current, &patch).unwrap();
        assert_eq!(cfg["ack_wait"], 1_000_000_000);
        assert!(changes.get("ack_wait").is_some());
        assert_ne!(
            revision(&json!({"config":current,"created":"a"})),
            revision(&json!({"config":current,"created":"b"}))
        );
        assert!(!supported("2.10.9"));
        assert!(supported("2.11.8"));
    }
}
