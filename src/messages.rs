use crate::{App, telemetry};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::Deserialize;
use serde_json::{Value, json};
use std::time::Duration;

type Failure = (StatusCode, String);

#[derive(Default, Deserialize)]
pub struct Browse {
    start: Option<u64>,
    subject: Option<String>,
}

fn bad(message: &str) -> Failure {
    (StatusCode::BAD_REQUEST, message.into())
}
fn upstream(message: &str) -> Failure {
    (StatusCode::BAD_GATEWAY, message.into())
}

pub fn valid_filter(subject: &str) -> bool {
    let tokens: Vec<_> = subject.split('.').collect();
    !subject.is_empty()
        && subject.len() <= 1024
        && !subject.chars().any(|c| c.is_whitespace() || c.is_control())
        && tokens.iter().enumerate().all(|(i, t)| {
            !t.is_empty()
                && (*t == "*" || (*t == ">" && i + 1 == tokens.len()) || !t.contains(['*', '>']))
        })
}

async fn inventory(app: &App, stream: &str) -> Result<Value, Failure> {
    if !telemetry::valid_token(stream) {
        return Err(bad("Choose a valid stream"));
    }
    if !app.demo {
        return Ok(Value::Null);
    }
    app.current
        .read()
        .await
        .streams
        .iter()
        .find(|s| s["config"]["name"].as_str() == Some(stream))
        .cloned()
        .ok_or((
            StatusCode::NOT_FOUND,
            "Stream is not in the observed inventory".into(),
        ))
}

async fn client(app: &App) -> Result<async_nats::Client, Failure> {
    app.nats.read().await.clone().ok_or((
        StatusCode::SERVICE_UNAVAILABLE,
        "NATS is unavailable".into(),
    ))
}

// Only JetStream's explicit no-message response establishes absence. Transport
// and authorization errors must not be presented as empty storage.
pub async fn lookup(
    client: &async_nats::Client,
    prefix: &str,
    stream: &str,
    body: Value,
) -> Result<Option<Value>, Failure> {
    let reply = tokio::time::timeout(
        Duration::from_secs(3),
        client.request(
            format!("{prefix}.STREAM.MSG.GET.{stream}"),
            body.to_string().into(),
        ),
    )
    .await
    .map_err(|_| upstream("Record lookup timed out; storage state is unknown"))?
    .map_err(|_| upstream("Record lookup failed; check NATS connectivity and read permissions"))?;
    if reply.payload.len() > 2_000_000 {
        return Err(upstream(
            "Record response exceeds the 2 MB inspection limit",
        ));
    }
    let value: Value =
        serde_json::from_slice(&reply.payload).map_err(|_| upstream("Invalid record response"))?;
    if value["error"]["err_code"] == 10037 {
        return Ok(None);
    }
    if value.get("error").is_some() {
        return Err(upstream(
            "NATS rejected record lookup; check read permissions and stream availability",
        ));
    }
    if !value["message"]["seq"].is_u64() || !value["message"]["subject"].is_string() {
        return Err(upstream("Incomplete record response"));
    }
    Ok(Some(value["message"].clone()))
}

fn demo_record(stream: &Value, seq: u64) -> Value {
    json!({"seq":seq,"subject":stream["config"]["subjects"][0],"data":"eyJkZW1vIjp0cnVlfQ=="})
}

pub async fn message(
    State(app): State<App>,
    Path((stream, sequence)): Path<(String, u64)>,
) -> Result<Json<Value>, Failure> {
    if sequence == 0 {
        return Err(bad("Choose a positive stream sequence"));
    }
    let info = inventory(&app, &stream).await?;
    let result = if app.demo {
        (info["state"]["messages"].as_u64().unwrap_or(0) > 0
            && sequence >= info["state"]["first_seq"].as_u64().unwrap_or(1)
            && sequence <= info["state"]["last_seq"].as_u64().unwrap_or(0))
        .then(|| demo_record(&info, sequence))
    } else {
        lookup(
            &client(&app).await?,
            &app.prefix,
            &stream,
            json!({"seq":sequence}),
        )
        .await?
    };
    let mut message = result.ok_or((StatusCode::NOT_FOUND, "NATS reports no retained record at this sequence. It may have expired, been deleted, or never been stored.".into()))?;
    message["seq"] = sequence.to_string().into();
    Ok(Json(
        json!({"demo":app.demo,"stream":stream,"observed_at":telemetry::now(),"message":message}),
    ))
}

pub async fn latest(
    State(app): State<App>,
    Path(stream): Path<String>,
    Query(query): Query<Browse>,
) -> Result<Json<Value>, Failure> {
    let info = inventory(&app, &stream).await?;
    let subject = query
        .subject
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(">");
    if !valid_filter(subject) {
        return Err(bad("Choose a valid NATS subject filter"));
    }
    let record = if app.demo {
        (info["state"]["messages"].as_u64().unwrap_or(0) > 0
            && matches_subject(
                subject,
                info["config"]["subjects"][0].as_str().unwrap_or(""),
            ))
        .then(|| demo_record(&info, info["state"]["last_seq"].as_u64().unwrap_or(0)))
    } else {
        lookup(
            &client(&app).await?,
            &app.prefix,
            &stream,
            json!({"last_by_subj":subject}),
        )
        .await?
    };
    let mut message = record.ok_or((
        StatusCode::NOT_FOUND,
        "No retained record matches this subject filter at this read.".into(),
    ))?;
    message["seq"] = message["seq"].as_u64().unwrap().to_string().into();
    Ok(Json(
        json!({"demo":app.demo,"stream":stream,"observed_at":telemetry::now(),"message":message}),
    ))
}

pub async fn browse(
    State(app): State<App>,
    Path(stream): Path<String>,
    Query(query): Query<Browse>,
) -> Result<Json<Value>, Failure> {
    let mut info = inventory(&app, &stream).await?;
    let subject = query
        .subject
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(">");
    if !valid_filter(subject) || query.start == Some(0) {
        return Err(bad(
            "Use a positive sequence and a NATS subject filter; * matches one token, final > matches one or more",
        ));
    }
    let client = if app.demo {
        None
    } else {
        Some(client(&app).await?)
    };
    if let Some(client) = &client {
        info = telemetry::request(
            client,
            format!("{}.STREAM.INFO.{stream}", app.prefix),
            json!({}),
        )
        .await
        .map_err(|_| upstream("Stream storage information is unavailable"))?;
    }
    let first = info["state"]["first_seq"].as_u64().unwrap_or(1).max(1);
    let last = info["state"]["last_seq"].as_u64().unwrap_or(0);
    let mut cursor = query.start.unwrap_or(first).max(first);
    let mut records = Vec::new();
    let mut exhausted = info["state"]["messages"] == 0 || cursor > last;
    let scan = async {
        let mut budget = 0;
        // Reserve the full per-response allowance before another lookup.
        while records.len() < 20 && !exhausted && budget <= 6_000_000 {
            let message = if let Some(client) = &client {
                lookup(
                    client,
                    &app.prefix,
                    &stream,
                    json!({"seq":cursor,"next_by_subj":subject}),
                )
                .await?
            } else if matches_subject(
                subject,
                info["config"]["subjects"][0].as_str().unwrap_or(""),
            ) {
                Some(demo_record(&info, cursor))
            } else {
                None
            };
            let Some(mut message) = message else {
                exhausted = true;
                break;
            };
            let seq = message["seq"]
                .as_u64()
                .ok_or(upstream("Invalid record sequence"))?;
            if seq < cursor {
                return Err(upstream("NATS returned an unexpected sequence"));
            }
            if seq > last {
                exhausted = true;
                break;
            }
            budget += message.to_string().len();
            if budget > 8_000_000 {
                break;
            }
            let data = message["data"].as_str().unwrap_or("");
            let bytes = data.len() / 4 * 3
                - data
                    .chars()
                    .rev()
                    .take_while(|c| *c == '=')
                    .count()
                    .min(data.len() / 4 * 3);
            message["bytes"] = bytes.into();
            message["seq"] = seq.to_string().into();
            message.as_object_mut().unwrap().remove("data");
            message.as_object_mut().unwrap().remove("hdrs");
            records.push(message);
            exhausted = seq >= last;
            cursor = seq.saturating_add(1);
        }
        Ok::<(), Failure>(())
    };
    tokio::time::timeout(Duration::from_secs(8), scan)
        .await
        .map_err(|_| upstream("Browse timed out. Narrow the subject filter or retry."))??;
    // String cursors preserve u64 sequences in browsers beyond Number precision.
    Ok(Json(
        json!({"demo":app.demo,"stream":stream,"subject":subject,"observed_at":telemetry::now(),"state":info["state"],"first_seq":first.to_string(),"last_seq":last.to_string(),"records":records,"next_seq":if exhausted {None} else {Some(cursor.to_string())},"exhausted":exhausted}),
    ))
}

fn matches_subject(filter: &str, subject: &str) -> bool {
    let parts: Vec<_> = subject.split('.').collect();
    for (i, token) in filter.split('.').enumerate() {
        if i >= parts.len() {
            return false;
        }
        if token == ">" {
            return true;
        }
        if token != "*" && token != parts[i] {
            return false;
        }
    }
    filter.split('.').count() == parts.len()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn subject_filters() {
        for bad in ["", "a..b", "a.>.b", "a*", "a b", "a\n"] {
            assert!(!valid_filter(bad));
        }
        for good in [">", "a.*", "a.>", "a.b"] {
            assert!(valid_filter(good));
        }
        assert!(matches_subject("a.*", "a.b"));
        assert!(!matches_subject("a.>", "a"));
        assert!(!matches_subject("a.*", "a.b.c"));
    }
}
