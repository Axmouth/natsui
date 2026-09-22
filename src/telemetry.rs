use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
pub fn valid_token(value: &str) -> bool {
    !value.is_empty()
        && value.len() < 256
        && !value
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || ".*>/\\".contains(c))
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Snapshot {
    pub observed_at: u64,
    pub scope: String,
    pub demo: bool,
    pub status: String,
    pub issues: Vec<String>,
    pub streams: Vec<Value>,
    pub consumers: Vec<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scenario: Option<Value>,
}
impl Snapshot {
    pub fn unavailable(reason: &str, scope: &str) -> Self {
        Self {
            observed_at: now(),
            scope: scope.into(),
            demo: false,
            status: "unavailable".into(),
            issues: vec![reason.into()],
            streams: vec![],
            consumers: vec![],
            scenario: None,
        }
    }
}

pub async fn request(
    client: &async_nats::Client,
    subject: String,
    body: Value,
) -> Result<Value, String> {
    let message = tokio::time::timeout(
        Duration::from_secs(3),
        client.request(subject, body.to_string().into()),
    )
    .await
    .map_err(|_| "Request timed out")?
    .map_err(|_| "NATS request failed")?;
    // Payloads remain on demand. A response cap bounds message inspection and
    // protects the dashboard from unexpectedly large management responses.
    if message.payload.len() > 2_000_000 {
        return Err("Response exceeds 2 MB inspection limit".into());
    }
    let value: Value =
        serde_json::from_slice(&message.payload).map_err(|_| "Invalid NATS response")?;
    if value.get("error").is_some() {
        return Err("NATS returned an API error (resource, permissions or feature support)".into());
    }
    Ok(value)
}

async fn list(
    client: &async_nats::Client,
    subject: String,
    key: &str,
    cap: usize,
) -> Result<(Vec<Value>, bool), String> {
    let mut items = Vec::new();
    loop {
        let value = request(client, subject.clone(), json!({"offset":items.len()})).await?;
        let page = value[key].as_array().cloned().unwrap_or_default();
        let total = value["total"].as_u64().ok_or("Missing inventory total")? as usize;
        if page.is_empty() {
            return Ok((items.clone(), items.len() < total));
        }
        let remaining = cap.saturating_sub(items.len());
        items.extend(page.into_iter().take(remaining));
        if items.len() >= total {
            return Ok((items, false));
        }
        if items.len() >= cap {
            return Ok((items, true));
        }
    }
}

pub async fn observe(client: &async_nats::Client, prefix: &str, scope: &str) -> Snapshot {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(18);
    let (streams, truncated) = match list(client, format!("{prefix}.STREAM.LIST"), "streams", 300)
        .await
    {
        Ok(result) => result,
        Err(_) => {
            return Snapshot::unavailable(
                "JetStream inventory unavailable. Connectivity, permissions or JetStream configuration may be responsible.",
                scope,
            );
        }
    };
    let mut issues = Vec::new();
    if truncated {
        issues.push(
            "Stream inventory capped at 300; summaries cover observed resources only.".into(),
        );
    }
    let mut consumers = Vec::new();
    for stream in &streams {
        let Some(name) = stream["config"]["name"].as_str().filter(|s| valid_token(s)) else {
            issues.push("A stream has an unsupported name or response shape.".into());
            continue;
        };
        let remaining = 2000usize.saturating_sub(consumers.len());
        if remaining == 0 {
            issues.push("Consumer inventory capped at 2,000.".into());
            break;
        }
        let result = tokio::time::timeout_at(
            deadline,
            list(
                client,
                format!("{prefix}.CONSUMER.LIST.{name}"),
                "consumers",
                remaining,
            ),
        )
        .await;
        let Ok(result) = result else {
            issues.push(
                "Consumer collection reached its time budget. Stream inventory is retained.".into(),
            );
            break;
        };
        match result {
            Ok((mut values, truncated)) => {
                for value in &mut values {
                    value["stream_name"] = json!(name);
                }
                consumers.extend(values);
                if truncated {
                    issues.push(format!("Consumer inventory incomplete for {name}."));
                }
            }
            Err(_) => issues.push(format!("Consumer information unavailable for {name}.")),
        }
    }
    Snapshot {
        observed_at: now(),
        scope: scope.into(),
        demo: false,
        status: if issues.is_empty() {
            "complete"
        } else {
            "partial"
        }
        .into(),
        issues,
        streams,
        consumers,
        scenario: None,
    }
}

pub fn summarize(snapshot: &Snapshot, threshold: u64) -> Value {
    if snapshot.status == "unavailable" {
        return json!({"largest":null,"behind":null,"streams":null,"consumers":null,"stored_bytes":null});
    }
    let largest = snapshot
        .consumers
        .iter()
        .filter(|c| c["num_pending"].is_u64())
        .max_by_key(|c| c["num_pending"].as_u64().unwrap_or(0));
    json!({"largest":largest.map(|c| json!({"name":c["name"],"stream":c["stream_name"],"pending":c["num_pending"],"ack_pending":c["num_ack_pending"]})),
        "behind": snapshot.consumers.iter().filter(|c| c["num_pending"].as_u64().is_some_and(|n| n > threshold)).count(),
        "threshold":threshold,"streams":snapshot.streams.len(),"consumers":snapshot.consumers.len(),
        "stored_bytes":snapshot.streams.iter().filter_map(|s| s["state"]["bytes"].as_u64()).sum::<u64>()})
}

#[cfg(test)]
pub fn demo(scope: &str) -> Snapshot {
    let streams = [("ORDERS", "orders.>", 128450, 84230000), ("PAYMENTS", "payments.>", 24680, 14230000), ("EVENTS", "events.>", 90812, 45670000)].into_iter().map(|(name, subject, messages, bytes)| json!({"config":{"name":name,"subjects":[subject],"retention":"limits","storage":"file","num_replicas":3,"max_age":86400000000000u64,"max_bytes":1073741824},"state":{"messages":messages,"bytes":bytes,"first_seq":1,"last_seq":messages,"consumer_count":2}})).collect();
    let consumers = [("ORDERS","billing-worker",82431,1204),("ORDERS","fulfilment",12340,32),("PAYMENTS","settlement",18120,400),("PAYMENTS","receipts",0,0),("EVENTS","warehouse",10450,64),("EVENTS","notifications",24,2)].into_iter().map(|(stream,name,pending,acks)|json!({"stream_name":stream,"name":name,"num_pending":pending,"num_ack_pending":acks,"num_redelivered":0,"num_waiting":1,"config":{"ack_policy":"explicit","max_ack_pending":if name=="billing-worker" {1500} else {1000},"max_deliver":5,"ack_wait":30000000000u64,"filter_subject":format!("{}.>",stream.to_lowercase())},"delivered":{"stream_seq":45000},"ack_floor":{"stream_seq":43000}})).collect();
    Snapshot {
        observed_at: now(),
        scope: scope.into(),
        demo: true,
        status: "complete".into(),
        issues: vec![],
        streams,
        consumers,
        scenario: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backlog_uses_maximum_not_sum_or_offsets() {
        let s = demo("test");
        let result = summarize(&s, 10000);
        assert_eq!(result["largest"]["pending"], 82431);
        assert_eq!(result["behind"], 4);
    }
    #[test]
    fn unavailable_is_not_healthy_zero() {
        let result = summarize(&Snapshot::unavailable("offline", "test"), 10000);
        assert!(result["behind"].is_null());
        assert!(result["stored_bytes"].is_null());
    }
    #[test]
    fn threshold_is_strict_and_scope_is_preserved() {
        let mut s = demo("account A");
        s.status = "partial".into();
        let result = summarize(&s, 82431);
        assert_eq!(result["behind"], 0);
        assert_eq!(s.scope, "account A");
    }
    #[test]
    fn management_subjects_reject_wildcards() {
        for s in ["", "foo.bar", "x.*", ">", "a b"] {
            assert!(!valid_token(s));
        }
        assert!(valid_token("ORDERS-1"));
    }

    #[tokio::test]
    #[ignore = "Requires NATSUI_TEST_SERVER pointing to a local nats-server executable"]
    async fn live_filtered_consumers_and_non_consuming_inspection() {
        struct Server(std::process::Child, std::path::PathBuf);
        impl Drop for Server {
            fn drop(&mut self) {
                let _ = self.0.kill();
                let _ = self.0.wait();
                let _ = std::fs::remove_dir_all(&self.1);
            }
        }
        let binary = std::env::var("NATSUI_TEST_SERVER").expect("NATSUI_TEST_SERVER");
        let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = reservation.local_addr().unwrap().port();
        let dir = std::env::temp_dir().join(format!("natsui-live-{}-{port}", std::process::id()));
        drop(reservation);
        let child = std::process::Command::new(binary)
            .args(["-a", "127.0.0.1", "-p", &port.to_string(), "-js", "-sd"])
            .arg(&dir)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn()
            .unwrap();
        let _server = Server(child, dir);
        let address = format!("nats://127.0.0.1:{port}");
        let mut connected = None;
        for _ in 0..40 {
            if let Ok(client) = async_nats::connect(&address).await {
                connected = Some(client);
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let client = connected.expect("Disposable NATS server did not start");
        use futures_util::StreamExt;
        let mut advisory_subscription = client.subscribe("$JS.EVENT.ADVISORY.>").await.unwrap();
        client.flush().await.unwrap();
        request(&client, "$JS.API.STREAM.CREATE.ORDERS".into(), json!({"name":"ORDERS","subjects":["orders.>"],"storage":"memory","retention":"limits"})).await.unwrap();
        let native_advisory = tokio::time::timeout(Duration::from_secs(3), async {
            while let Some(message) = advisory_subscription.next().await {
                if let Ok(value) = serde_json::from_slice::<Value>(&message.payload)
                    && let Some(event) = crate::incidents::advisory(&value)
                {
                    return event;
                }
            }
            panic!("Advisory subscription closed");
        })
        .await
        .unwrap();
        assert_eq!(native_advisory["stream"], "ORDERS");
        advisory_subscription.unsubscribe().await.unwrap();
        for name in ["alpha", "beta"] {
            request(&client, format!("$JS.API.CONSUMER.DURABLE.CREATE.ORDERS.{name}"), json!({"stream_name":"ORDERS","config":{"durable_name":name,"ack_policy":"explicit","filter_subject":"orders.created"}})).await.unwrap();
        }
        for subject in [
            "orders.created",
            "orders.created",
            "orders.created",
            "orders.ignored",
            "orders.ignored",
        ] {
            let reply = client
                .request(subject, "{\"test\":true}".into())
                .await
                .unwrap();
            assert!(serde_json::from_slice::<Value>(&reply.payload).unwrap()["seq"].is_u64());
        }
        let before = observe(&client, "$JS.API", "integration").await;
        assert_eq!(before.status, "complete");
        assert_eq!(before.streams[0]["state"]["messages"], 5);
        let summary = summarize(&before, 2);
        assert_eq!(summary["largest"]["pending"], 3);
        assert_eq!(summary["behind"], 2);
        let message = request(
            &client,
            "$JS.API.STREAM.MSG.GET.ORDERS".into(),
            json!({"seq":1}),
        )
        .await
        .unwrap();
        assert_eq!(message["message"]["subject"], "orders.created");
        let after = observe(&client, "$JS.API", "integration").await;
        for consumer in &after.consumers {
            assert_eq!(consumer["num_pending"], 3);
            assert_eq!(consumer["num_ack_pending"], 0);
            assert_eq!(consumer["delivered"]["consumer_seq"], 0);
        }
        assert!(
            crate::messages::lookup(&client, "$JS.API", "ORDERS", json!({"seq":999}))
                .await
                .unwrap()
                .is_none()
        );
        request(
            &client,
            "$JS.API.STREAM.MSG.DELETE.ORDERS".into(),
            json!({"seq":2}),
        )
        .await
        .unwrap();
        assert!(
            crate::messages::lookup(&client, "$JS.API", "MISSING", json!({"seq":1}))
                .await
                .is_err()
        );
        let before_browse = observe(&client, "$JS.API", "integration").await;
        let app = crate::App {
            auth: crate::auth::Auth::disabled(),
            editor: crate::editing::Editor::default(),
            connection: None,
            settings_cache: std::sync::Arc::new(tokio::sync::RwLock::new(
                crate::store::Settings::default(),
            )),
            db: crate::store::Database::open(_server.1.join("dashboard").to_str().unwrap())
                .unwrap(),
            current: std::sync::Arc::new(tokio::sync::RwLock::new(before_browse.clone())),
            nats: std::sync::Arc::new(tokio::sync::RwLock::new(Some(client.clone()))),
            monitor: crate::monitoring::Monitor::new("").unwrap(),
            demo: false,
            prefix: "$JS.API".into(),
            scope: "integration".into(),
        };
        let query = serde_json::from_value(json!({"subject":"orders.*","start":1})).unwrap();
        let page = crate::messages::browse(
            axum::extract::State(app.clone()),
            axum::extract::Path("ORDERS".into()),
            axum::extract::Query(query),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(
            page["records"]
                .as_array()
                .unwrap()
                .iter()
                .map(|r| r["seq"].as_str().unwrap())
                .collect::<Vec<_>>(),
            vec!["1", "3", "4", "5"]
        );
        assert!(page["records"][0].get("data").is_none());
        assert_eq!(page["exhausted"], true);
        let query = serde_json::from_value(json!({"subject":"orders.ignored"})).unwrap();
        let page = crate::messages::browse(
            axum::extract::State(app.clone()),
            axum::extract::Path("ORDERS".into()),
            axum::extract::Query(query),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(page["records"].as_array().unwrap().len(), 2);
        let query = serde_json::from_value(json!({"subject":"orders.*"})).unwrap();
        let latest = crate::messages::latest(
            axum::extract::State(app.clone()),
            axum::extract::Path("ORDERS".into()),
            axum::extract::Query(query),
        )
        .await
        .unwrap()
        .0;
        assert_eq!(latest["message"]["seq"], "5");
        let missing = crate::messages::message(
            axum::extract::State(app),
            axum::extract::Path(("ORDERS".into(), 2)),
        )
        .await
        .unwrap_err();
        assert_eq!(missing.0, axum::http::StatusCode::NOT_FOUND);
        let after_browse = observe(&client, "$JS.API", "integration").await;
        for before in &before_browse.consumers {
            let after = after_browse
                .consumers
                .iter()
                .find(|c| c["name"] == before["name"])
                .unwrap();
            for field in ["delivered", "ack_floor", "num_ack_pending", "num_pending"] {
                assert_eq!(before[field], after[field]);
            }
        }
        let invalid = observe(&client, "$JS.missing.API", "integration").await;
        assert_eq!(invalid.status, "unavailable");
    }
}
