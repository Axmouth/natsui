use crate::{
    App,
    telemetry::{Snapshot, now},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::time::Duration;

fn event(at: u64, kind: &str, resource: &str, stream: &str, name: &str, detail: String) -> Value {
    json!({"at":at,"kind":kind,"resource":resource,"stream":stream,"name":name,"detail":detail,"source":"observed change"})
}

// Comparison is restricted to public operational settings. Descriptions,
// metadata and credentials never enter the incident store.
fn configuration(value: &Value) -> Value {
    let mut result = serde_json::Map::new();
    for key in [
        "subjects",
        "retention",
        "storage",
        "num_replicas",
        "max_msgs",
        "max_bytes",
        "max_age",
        "discard",
        "ack_policy",
        "ack_wait",
        "max_ack_pending",
        "max_deliver",
        "filter_subject",
        "filter_subjects",
        "backoff",
        "deliver_policy",
        "replay_policy",
        "inactive_threshold",
    ] {
        if let Some(v) = value.get(key) {
            result.insert(key.into(), v.clone());
        }
    }
    Value::Object(result)
}

pub fn changes(
    previous: &Snapshot,
    current: &Snapshot,
    old_nodes: &Value,
    nodes: &Value,
    threshold: u64,
) -> Vec<Value> {
    let mut result = vec![];
    let at = current.observed_at;
    if previous.status == "complete" && current.status == "complete" {
        for (kind, before, after) in [
            ("stream", &previous.streams, &current.streams),
            ("consumer", &previous.consumers, &current.consumers),
        ] {
            let name_of = |v: &Value| {
                if kind == "stream" {
                    v["config"]["name"].as_str().unwrap_or("").to_owned()
                } else {
                    v["name"].as_str().unwrap_or("").to_owned()
                }
            };
            for item in after {
                let name = name_of(item);
                let stream = if kind == "stream" {
                    name.clone()
                } else {
                    item["stream_name"].as_str().unwrap_or("").into()
                };
                let old = before.iter().find(|v| {
                    name_of(v) == name
                        && (kind == "stream" || v["stream_name"] == item["stream_name"])
                });
                if let Some(old) = old {
                    if old["created"] != item["created"] {
                        result.push(event(
                            at,
                            "recreated",
                            kind,
                            &stream,
                            &name,
                            "Resource creation identity changed".into(),
                        ));
                        continue;
                    }
                    let a = configuration(&old["config"]);
                    let b = configuration(&item["config"]);
                    if a != b {
                        let mut changes = serde_json::Map::new();
                        for key in a
                            .as_object()
                            .unwrap()
                            .keys()
                            .chain(b.as_object().unwrap().keys())
                        {
                            if a[key] != b[key] {
                                changes
                                    .insert(key.clone(), json!({"before":a[key],"after":b[key]}));
                            }
                        }
                        let mut e = event(
                            at,
                            "configuration",
                            kind,
                            &stream,
                            &name,
                            format!(
                                "Configuration changed: {}",
                                changes.keys().cloned().collect::<Vec<_>>().join(", ")
                            ),
                        );
                        e["changes"] = Value::Object(changes);
                        result.push(e);
                    }
                    if kind == "consumer" {
                        if let (Some(a), Some(b)) =
                            (old["num_pending"].as_u64(), item["num_pending"].as_u64())
                            && (a > threshold) != (b > threshold)
                        {
                            result.push(event(
                                at,
                                if b > threshold {
                                    "backlog-high"
                                } else {
                                    "backlog-recovered"
                                },
                                kind,
                                &stream,
                                &name,
                                format!("Pending delivery {a} to {b}; threshold {threshold}"),
                            ));
                        }
                        let pressure = |c: &Value| {
                            c["num_ack_pending"]
                                .as_f64()
                                .zip(c["config"]["max_ack_pending"].as_f64())
                                .filter(|(_, m)| *m > 0.0)
                                .map(|(n, m)| n / m >= 0.8)
                        };
                        if let (Some(a), Some(b)) = (pressure(old), pressure(item))
                            && a != b
                        {
                            result.push(event(
                                at,
                                if b { "ack-pressure" } else { "ack-recovered" },
                                kind,
                                &stream,
                                &name,
                                format!(
                                    "Acknowledgment capacity {} 80%",
                                    if b { "reached" } else { "fell below" }
                                ),
                            ));
                        }
                    }
                } else {
                    result.push(event(
                        at,
                        "discovered",
                        kind,
                        &stream,
                        &name,
                        "Resource entered the observed inventory".into(),
                    ));
                }
            }
            for old in before {
                let name = name_of(old);
                if !after.iter().any(|v| {
                    name_of(v) == name
                        && (kind == "stream" || v["stream_name"] == old["stream_name"])
                }) {
                    result.push(event(at,"not-observed",kind,if kind=="stream" {&name} else {old["stream_name"].as_str().unwrap_or("")},&name,"Resource left the observed inventory; deletion is not established by inventory alone".into()));
                }
            }
        }
    }
    if let (Some(before), Some(after)) = (old_nodes["nodes"].as_array(), nodes["nodes"].as_array())
    {
        for n in after.iter().filter(|n| n["status"] == "complete") {
            if let Some(old) = before.iter().find(|v| v["slot"] == n["slot"]) {
                let name = n["server_name"].as_str().unwrap_or("Unnamed node");
                if !old["start"].is_null()
                    && (old["start"] != n["start"] || old["server_id"] != n["server_id"])
                {
                    result.push(event(
                        at,
                        "restart-or-replacement",
                        "node",
                        "",
                        name,
                        "Startup identity changed at the configured endpoint".into(),
                    ));
                } else if !old["config_load_time"].is_null()
                    && old["config_load_time"] != n["config_load_time"]
                {
                    result.push(event(
                        at,
                        "configuration-reload",
                        "node",
                        "",
                        name,
                        "Reported configuration load time changed".into(),
                    ));
                }
            }
        }
    }
    result
}

pub fn advisory(value: &Value) -> Option<Value> {
    let kind = value["type"].as_str()?;
    if !kind.starts_with("io.nats.jetstream.advisory.")
        || kind.ends_with(".api_audit")
        || kind.ends_with(".nak")
    {
        return None;
    }
    let mut e = json!({"at":now(),"kind":"advisory","source":"NATS advisory","detail":kind,"resource":if value["consumer"].is_string(){"consumer"}else{"stream"},"stream":value["stream"],"name":value["consumer"].as_str().or(value["stream"].as_str()).unwrap_or("")});
    for key in [
        "id",
        "timestamp",
        "action",
        "stream_seq",
        "deliveries",
        "leader",
    ] {
        if value[key].is_string() || value[key].is_u64() {
            e[key] = value[key].clone();
        }
    }
    Some(e)
}

pub async fn collect(app: App) {
    if app.demo {
        return;
    }
    let domain = std::env::var("NATSUI_DOMAIN").unwrap_or_default();
    loop {
        let Some(client) = app.nats.read().await.clone() else {
            tokio::time::sleep(Duration::from_secs(2)).await;
            continue;
        };
        let Ok(mut subscription) = client.subscribe("$JS.EVENT.ADVISORY.>").await else {
            tokio::time::sleep(Duration::from_secs(5)).await;
            continue;
        };
        let _ = client.flush().await;
        let mut state = String::new();
        let mut tick = tokio::time::interval(Duration::from_secs(2));
        let mut count = 0;
        let mut window = now();
        let mut dropped = false;
        let start = json!({"at":now(),"kind":"advisory-coverage","source":"dashboard","detail":"Advisory listener started. API audit and per-message NAK events are excluded. Delivery permissions are not verified; events before subscription or during outages cannot be recovered."});
        let _ = app.db.incidents(&app.scope, false, vec![start]).await;
        loop {
            tokio::select! {
                _=tick.tick()=>{
                    let next=client.connection_state().to_string();
                    if next!=state {let _=app.db.incidents(&app.scope,false,vec![json!({"at":now(),"kind":"advisory-coverage","source":"dashboard","detail":format!("Advisory connection: {next}. Subscription delivery remains permission-dependent.")})]).await;state=next;}
                }
                message=subscription.next()=>{
                    let Some(message)=message else {break;};
                    if now().saturating_sub(window)>=60 {window=now();count=0;dropped=false;}
                    if message.subject.as_str().contains(".API") || message.subject.as_str().contains(".MSG_NAKED.") {continue;}
                    if count>=120 || message.payload.len()>65536 {
                        if !dropped {let _=app.db.incidents(&app.scope,false,vec![json!({"at":now(),"kind":"advisory-gap","source":"dashboard","detail":"Advisory recording cap reached or oversized advisory skipped. This interval is incomplete."})]).await;dropped=true;}
                        continue;
                    }
                    if let Ok(value)=serde_json::from_slice::<Value>(&message.payload)&& (domain.is_empty() || value["domain"].as_str()==Some(domain.as_str())) && let Some(event)=advisory(&value) {count+=1;if let Err(error)=app.db.incidents(&app.scope,false,vec![event]).await {eprintln!("Advisory storage failed: {error}");}}
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(5)).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn changes_respect_gaps_and_remove_private_metadata() {
        let old = crate::simulation::snapshot("test", 0);
        let mut new = old.clone();
        new.consumers[0]["num_pending"] = 20000.into();
        new.streams[0]["config"]["max_msgs"] = 123.into();
        new.streams[0]["config"]["metadata"] = json!({"secret":"hidden"});
        let events = changes(&old, &new, &json!({}), &json!({}), 10000);
        assert!(events.iter().any(|e| e["kind"] == "backlog-high"));
        assert!(events.iter().any(|e| e["kind"] == "configuration"));
        assert!(!serde_json::to_string(&events).unwrap().contains("secret"));
        new.status = "partial".into();
        assert!(changes(&old, &new, &json!({}), &json!({}), 10000).is_empty());
        let e=advisory(&json!({"type":"io.nats.jetstream.advisory.v1.max_deliver","stream":"S","consumer":"C","data":"secret"})).unwrap();
        assert!(e.get("data").is_none());
        assert!(advisory(&json!({"type":"io.nats.jetstream.advisory.v1.api_audit"})).is_none());
    }
}
