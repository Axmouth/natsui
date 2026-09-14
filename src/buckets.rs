use crate::{App, messages, telemetry};
use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use base64::{
    Engine,
    engine::general_purpose::{STANDARD, URL_SAFE, URL_SAFE_NO_PAD},
};
use serde::Deserialize;
use serde_json::{Value, json};
type Error = (StatusCode, String);
fn bad(message: &str) -> Error {
    (StatusCode::BAD_REQUEST, message.into())
}
fn target(kind: &str, bucket: &str) -> Result<(String, String), Error> {
    if !telemetry::valid_token(bucket) || bucket.len() > 128 {
        return Err(bad("Invalid bucket name"));
    }
    match kind {
        "kv" => Ok((format!("KV_{bucket}"), format!("$KV.{bucket}."))),
        "object" => Ok((format!("OBJ_{bucket}"), format!("$O.{bucket}.M."))),
        _ => Err(bad("Choose KV or Object Store")),
    }
}
async fn client(app: &App) -> Result<async_nats::Client, Error> {
    app.nats.read().await.clone().ok_or_else(|| {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            "NATS is disconnected".into(),
        )
    })
}
pub async fn buckets(State(app): State<App>) -> Json<Value> {
    let state = app.current.read().await;
    let rows:Vec<Value>=state.streams.iter().filter_map(|stream|{
        let name=stream["config"]["name"].as_str()?;
        let (kind,bucket)=if let Some(bucket)=name.strip_prefix("KV_"){("kv",bucket)}else{("object",name.strip_prefix("OBJ_")?)};
        Some(json!({"kind":kind,"bucket":bucket,"stream":name,"bytes":stream["state"]["bytes"],"storage":stream["config"]["storage"],"replicas":stream["config"]["num_replicas"]}))
    }).collect();
    Json(json!({"buckets":rows,"status":state.status,"observed_at":state.observed_at}))
}
#[derive(Deserialize)]
pub struct Page {
    #[serde(default)]
    offset: usize,
}
pub async fn keys(
    State(app): State<App>,
    Path((kind, bucket)): Path<(String, String)>,
    Query(page): Query<Page>,
) -> Result<Json<Value>, Error> {
    if page.offset > 1_000_000 {
        return Err(bad("Bucket offset exceeds the inspection limit"));
    }
    let (stream, prefix) = target(&kind, &bucket)?;
    let raw = telemetry::request(
        &client(&app).await?,
        format!("{}.STREAM.INFO.{stream}", app.prefix),
        json!({"subjects_filter":format!("{prefix}>"),"offset":page.offset}),
    )
    .await
    .map_err(|e| (StatusCode::BAD_GATEWAY, e))?;
    let subjects = raw["state"]["subjects"].as_object();
    let mut subjects: Vec<_> = subjects.map(|s| s.iter().collect()).unwrap_or_default();
    subjects.sort_by_key(|(key, _)| *key);
    let rows:Vec<Value>=subjects.iter().take(100).filter_map(|(subject,count)|{
        let suffix=subject.strip_prefix(&prefix)?;
        let name=if kind=="object"{URL_SAFE.decode(suffix).or_else(|_| URL_SAFE_NO_PAD.decode(suffix)).ok().and_then(|b|String::from_utf8(b).ok()).unwrap_or_else(||suffix.into())}else{suffix.into()};
        Some(json!({"key":suffix,"name":name,"retained_revisions":count,"presence":"Retained subject. Inspect the latest revision to distinguish a value from a tombstone."}))
    }).collect();
    let next = page.offset + rows.len();
    let total = raw["total"].as_u64();
    let has_more =
        subjects.len() > rows.len() || total.is_some_and(|total| next < (total as usize));
    Ok(Json(
        json!({"kind":kind,"bucket":bucket,"rows":rows,"offset":page.offset,"next_offset":if has_more{Some(next)}else{None},"total_subjects":total,"limit":100,"snapshot":false}),
    ))
}
#[derive(Deserialize)]
pub struct Entry {
    key: String,
}
pub async fn entry(
    State(app): State<App>,
    Path((kind, bucket)): Path<(String, String)>,
    Query(query): Query<Entry>,
) -> Result<Json<Value>, Error> {
    let (stream, prefix) = target(&kind, &bucket)?;
    if query.key.len() > 1024
        || !messages::valid_filter(&query.key)
        || query.key.contains(['*', '>'])
    {
        return Err(bad("Choose one exact key or object metadata subject"));
    }
    let record = messages::lookup(
        &client(&app).await?,
        &app.prefix,
        &stream,
        json!({"last_by_subj":format!("{prefix}{}",query.key)}),
    )
    .await?;
    let Some(record) = record else {
        return Ok(Json(
            json!({"present":false,"reason":"No retained revision"}),
        ));
    };
    let data = record["data"].as_str().unwrap_or("");
    let bytes = STANDARD.decode(data).map_err(|_| {
        (
            StatusCode::BAD_GATEWAY,
            "Invalid stored payload encoding".into(),
        )
    })?;
    if bytes.len() > 65536 {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            "Value exceeds the 64 KiB preview limit".into(),
        ));
    }
    if kind == "object" {
        let meta: Value = serde_json::from_slice(&bytes)
            .map_err(|_| (StatusCode::BAD_GATEWAY, "Invalid object metadata".into()))?;
        let mut projected = json!({"present":meta["deleted"]!=true,"kind":"object","revision":record["seq"],"time":record["time"]});
        for field in [
            "name", "bucket", "size", "chunks", "digest", "deleted", "mtime", "options",
        ] {
            projected[field] = meta[field].clone();
        }
        return Ok(Json(projected));
    }
    let headers = STANDARD
        .decode(record["hdrs"].as_str().unwrap_or(""))
        .map_err(|_| (StatusCode::BAD_GATEWAY, "Invalid KV headers".into()))?;
    let operation = String::from_utf8_lossy(&headers)
        .lines()
        .find_map(|line| {
            line.split_once(':')
                .filter(|(name, _)| name.eq_ignore_ascii_case("KV-Operation"))
                .map(|(_, value)| value.trim().to_owned())
        })
        .unwrap_or("PUT".into());
    Ok(Json(
        json!({"kind":"kv","key":query.key,"present":operation=="PUT","operation":operation,"revision":record["seq"],"time":record["time"],"bytes":bytes.len(),"text":std::str::from_utf8(&bytes).ok(),"base64":data}),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bucket_names_cannot_escape_the_selected_namespace() {
        for name in ["", "a.b", "*", ">", "a/b"] {
            assert!(target("kv", name).is_err());
        }
        assert_eq!(
            target("object", "images").unwrap(),
            ("OBJ_images".into(), "$O.images.M.".into())
        );
    }
}
