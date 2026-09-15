use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct Monitor {
    clients: Vec<reqwest::Client>,
    credentials: Vec<HttpCredentials>,
    urls: Vec<reqwest::Url>,
    pub current: Arc<RwLock<Value>>,
}
#[derive(Clone, Default)]
enum HttpCredentials {
    #[default]
    None,
    Basic(String, String),
    Bearer(String),
}
#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct Endpoint {
    url: String,
    ca_file: Option<String>,
    cert_file: Option<String>,
    key_file: Option<String>,
    username: Option<String>,
    password_file: Option<String>,
    bearer_file: Option<String>,
}
fn private_file(path: &str, limit: u64) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "Monitoring configuration file unavailable")?
        .take(limit + 1)
        .read_to_end(&mut bytes)?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err("Monitoring configuration file is empty or oversized".into());
    }
    Ok(bytes)
}
fn secret(path: &str) -> Result<String, Box<dyn std::error::Error>> {
    let bytes = private_file(path, 8192)?;
    let value = String::from_utf8(bytes)
        .map_err(|_| "Monitoring credential must be UTF-8")?
        .trim_end_matches(['\r', '\n'])
        .to_owned();
    if value.is_empty() || value.chars().any(char::is_control) {
        return Err("Invalid monitoring credential".into());
    }
    Ok(value)
}
fn http_client() -> reqwest::ClientBuilder {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(3))
        .connect_timeout(Duration::from_secs(2))
        .redirect(reqwest::redirect::Policy::none())
        .no_proxy()
}
impl Monitor {
    pub fn from_file(path: &str) -> Result<Self, Box<dyn std::error::Error>> {
        Self::configured(&private_file(path, 65536)?)
    }
    pub fn from_env() -> Result<Self, Box<dyn std::error::Error>> {
        let urls = std::env::var("NATSUI_MONITOR_URLS").unwrap_or_default();
        match std::env::var("NATSUI_MONITOR_CONFIG_FILE") {
            Ok(path) => {
                if !urls.trim().is_empty() {
                    return Err(
                        "Use either NATSUI_MONITOR_URLS or NATSUI_MONITOR_CONFIG_FILE".into(),
                    );
                }
                Self::configured(&private_file(&path, 65536)?)
            }
            Err(std::env::VarError::NotPresent) => Self::new(&urls),
            Err(_) => Err("Invalid NATSUI_MONITOR_CONFIG_FILE".into()),
        }
    }
    fn configured(bytes: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        let endpoints: Vec<Endpoint> = serde_json::from_slice(bytes)
            .map_err(|_| "Invalid monitoring endpoint configuration")?;
        if endpoints.is_empty() || endpoints.len() > 32 {
            return Err("Configure between 1 and 32 monitoring endpoints".into());
        }
        let mut monitor = Self::new("")?;
        for endpoint in endpoints {
            let urls = parse_urls(&endpoint.url)?;
            if urls.len() != 1 {
                return Err("Each monitoring entry must specify one origin".into());
            }
            let url = urls.into_iter().next().unwrap();
            let has_tls = endpoint.ca_file.is_some()
                || endpoint.cert_file.is_some()
                || endpoint.key_file.is_some();
            let has_auth = endpoint.username.is_some()
                || endpoint.password_file.is_some()
                || endpoint.bearer_file.is_some();
            if (has_tls || has_auth) && url.scheme() != "https" {
                return Err("Monitoring TLS settings and credentials require HTTPS".into());
            }
            if endpoint.cert_file.is_some() != endpoint.key_file.is_some() {
                return Err(
                    "Monitoring client certificate and key must be configured together".into(),
                );
            }
            if endpoint.username.is_some() != endpoint.password_file.is_some()
                || (endpoint.bearer_file.is_some() && endpoint.username.is_some())
            {
                return Err(
                    "Configure a monitoring username and password file, or a bearer file".into(),
                );
            }
            let mut client = http_client();
            if let Some(path) = endpoint.ca_file {
                let roots = reqwest::Certificate::from_pem_bundle(&private_file(&path, 1048576)?)
                    .map_err(|_| "Invalid monitoring CA bundle")?;
                if roots.is_empty() {
                    return Err("Monitoring CA bundle has no certificates".into());
                }
                for root in roots {
                    client = client.add_root_certificate(root);
                }
            }
            if let (Some(cert), Some(key)) = (endpoint.cert_file, endpoint.key_file) {
                let mut pem = private_file(&cert, 1048576)?;
                pem.push(b'\n');
                pem.extend(private_file(&key, 1048576)?);
                client = client.identity(
                    reqwest::Identity::from_pem(&pem)
                        .map_err(|_| "Invalid monitoring client identity")?,
                );
            }
            let credentials = if let Some(path) = endpoint.bearer_file {
                HttpCredentials::Bearer(secret(&path)?)
            } else if let (Some(username), Some(path)) = (endpoint.username, endpoint.password_file)
            {
                if username.is_empty()
                    || username.contains(':')
                    || username.chars().any(char::is_control)
                {
                    return Err("Invalid monitoring username".into());
                }
                HttpCredentials::Basic(username, secret(&path)?)
            } else {
                HttpCredentials::None
            };
            monitor.urls.push(url);
            monitor.clients.push(
                client
                    .build()
                    .map_err(|_| "Monitoring HTTP client configuration failed")?,
            );
            monitor.credentials.push(credentials);
        }
        monitor.current = Arc::new(RwLock::new(json!({"status":"connecting","nodes":[]})));
        Ok(monitor)
    }

    pub async fn inventory(&self, kind: &str, page: usize) -> Result<Value, String> {
        if !["connections", "subscriptions", "routes"].contains(&kind) || page > 10000 {
            return Err("Unsupported inventory or page".into());
        }
        let current = self.current.read().await.clone();
        let mut tasks = tokio::task::JoinSet::new();
        for index in 0..self.urls.len() {
            let monitor = self.clone();
            let kind = kind.to_owned();
            let identity = current["nodes"]
                .as_array()
                .and_then(|v| v.iter().find(|n| n["slot"] == index))
                .cloned()
                .unwrap_or(json!({}));
            tasks.spawn(async move {
                let path = if kind=="routes" {"routez?subs=0".into()} else if kind=="connections" {format!("connz?subs=1&limit=100&offset={}",page*100)} else {format!("subsz?subs=1&limit=100&offset={}",page*100)};
                let mut node=json!({"slot":index,"name":identity["server_name"],"server_id":identity["server_id"],"start":identity["start"],"at":crate::telemetry::now()});
                match monitor.get(index,&path).await.and_then(|raw| project_inventory(&kind,&raw)) {
                    Ok(result)=>{if kind=="routes" {node["server_id"]=result["server_id"].clone();node["name"]=result["server_name"].clone();node["start"]=Value::Null;}node["status"]=if result["truncated"]==true {"partial"}else{"complete"}.into();node["rows"]=result["rows"].clone();node["total"]=result["total"].clone();}
                    Err(error)=>{node["status"]="unavailable".into();node["error"]=error.into();}
                }
                node
            });
        }
        let mut nodes = vec![];
        while let Some(result) = tasks.join_next().await {
            nodes.push(result.map_err(|_| "Inventory task failed")?);
        }
        nodes.sort_by_key(|n| n["slot"].as_u64());
        Ok(
            json!({"kind":kind,"page":page,"limit":100,"nodes":nodes,"at":crate::telemetry::now(),"scope":"Configured monitoring endpoints; may include accounts outside the JetStream profile"}),
        )
    }
    pub fn new(config: &str) -> Result<Self, Box<dyn std::error::Error>> {
        let urls = parse_urls(config)?;
        let status = if urls.is_empty() {
            "not_configured"
        } else {
            "connecting"
        };
        Ok(Self {
            clients: vec![http_client().build()?; urls.len()],
            credentials: vec![HttpCredentials::None; urls.len()],
            urls,
            current: Arc::new(RwLock::new(json!({"status":status,"nodes":[]}))),
        })
    }
    async fn get(&self, index: usize, path: &str) -> Result<Value, String> {
        let base = self.urls.get(index).ok_or("Unknown monitoring endpoint")?;
        let url = base.join(path).map_err(|_| "Invalid monitoring path")?;
        let request = self.clients[index].get(url);
        let request = match &self.credentials[index] {
            HttpCredentials::None => request,
            HttpCredentials::Basic(user, password) => request.basic_auth(user, Some(password)),
            HttpCredentials::Bearer(token) => request.bearer_auth(token),
        };
        let mut response = request
            .send()
            .await
            .map_err(|_| "Monitoring endpoint unavailable")?
            .error_for_status()
            .map_err(|_| "Monitoring request rejected")?;
        let mut body = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "Monitoring response failed")?
        {
            if body.len() + chunk.len() > 2_000_000 {
                return Err("Monitoring response exceeds 2 MB".into());
            }
            body.extend_from_slice(&chunk);
        }
        serde_json::from_slice(&body).map_err(|_| "Invalid monitoring response".into())
    }
    pub async fn collect(self) {
        if self.urls.is_empty() {
            return;
        }
        loop {
            let mut tasks = tokio::task::JoinSet::new();
            for index in 0..self.urls.len() {
                let monitor = self.clone();
                tasks.spawn(async move {
                    let at = crate::telemetry::now();
                    match monitor.get(index, "varz").await {
                        Ok(value) => project_node(&value, index, at),
                        Err(error) => {
                            json!({"slot":index,"status":"unavailable","at":at,"error":error})
                        }
                    }
                });
            }
            let mut nodes = vec![];
            while let Some(result) = tasks.join_next().await {
                if let Ok(value) = result {
                    nodes.push(value);
                }
            }
            nodes.sort_by_key(|n| n["slot"].as_u64());
            // Preserve identity across an outage so historical drilldowns remain
            // reachable. Current measurements are never copied forward.
            let previous = self.current.read().await.clone();
            for node in nodes.iter_mut().filter(|n| n["status"] != "complete") {
                if let Some(known) = previous["nodes"]
                    .as_array()
                    .and_then(|rows| rows.iter().find(|row| row["slot"] == node["slot"]))
                {
                    for key in ["server_id", "server_name", "start", "version"] {
                        node[key] = known[key].clone();
                    }
                }
            }
            let complete = nodes.iter().all(|n| n["status"] == "complete");
            *self.current.write().await =
                json!({"status":if complete{"complete"}else{"partial"},"nodes":nodes});
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    pub async fn connections(&self, index: usize) -> Result<Value, String> {
        let raw = self.get(index, "connz?limit=100").await?;
        let rows = raw["connections"]
            .as_array()
            .ok_or("Invalid connection inventory")?;
        let connections: Vec<Value> = rows
            .iter()
            .map(|c| {
                let mut v = serde_json::Map::new();
                for key in [
                    "cid",
                    "name",
                    "lang",
                    "version",
                    "rtt",
                    "pending_bytes",
                    "subscriptions",
                    "in_msgs",
                    "out_msgs",
                ] {
                    v.insert(key.into(), c[key].clone());
                }
                Value::Object(v)
            })
            .collect();
        Ok(
            json!({"total":raw["total"],"connections":connections,"limit":100,"at":crate::telemetry::now()}),
        )
    }
}
fn project_inventory(kind: &str, raw: &Value) -> Result<Value, String> {
    if kind == "routes" {
        let rows = raw["routes"]
            .as_array()
            .ok_or("Route inventory unavailable")?;
        let total = raw["num_routes"]
            .as_u64()
            .ok_or("Route count unavailable")?;
        let rows: Vec<Value> = rows
            .iter()
            .take(100)
            .map(|row| {
                let mut projected = serde_json::Map::new();
                for key in [
                    "rid",
                    "remote_id",
                    "remote_name",
                    "did_solicit",
                    "is_configured",
                    "start",
                    "rtt",
                    "pending_size",
                    "in_msgs",
                    "out_msgs",
                    "in_bytes",
                    "out_bytes",
                    "subscriptions",
                ] {
                    if let Some(value) = row.get(key) {
                        projected.insert(key.into(), value.clone());
                    }
                }
                Value::Object(projected)
            })
            .collect();
        return Ok(
            json!({"server_id":raw["server_id"],"server_name":raw["server_name"],"total":total,"truncated":total>rows.len() as u64,"rows":rows}),
        );
    }
    let key = if kind == "connections" {
        "connections"
    } else {
        "subscriptions_list"
    };
    let rows = raw[key].as_array().ok_or("Inventory rows unavailable")?;
    let total = raw["total"].as_u64().ok_or("Inventory total unavailable")?;
    let keys: &[&str] = if kind == "connections" {
        &[
            "cid",
            "name",
            "lang",
            "version",
            "start",
            "rtt",
            "pending_bytes",
            "subscriptions",
            "in_msgs",
            "out_msgs",
            "in_bytes",
            "out_bytes",
            "account",
        ]
    } else {
        &["subject", "qgroup", "cid", "sid", "msgs", "account"]
    };
    let rows: Vec<Value> = rows
        .iter()
        .take(100)
        .map(|row| {
            let mut v = serde_json::Map::new();
            for key in keys {
                if let Some(value) = row.get(*key)
                    && (value.is_string() || value.is_number())
                {
                    v.insert((*key).into(), value.clone());
                }
            }
            if kind == "connections"
                && let Some(subs) = row["subscriptions_list"].as_array()
            {
                v.insert(
                    "subjects".into(),
                    json!(
                        subs.iter()
                            .filter_map(Value::as_str)
                            .take(200)
                            .collect::<Vec<_>>()
                    ),
                );
                v.insert("subjects_truncated".into(), (subs.len() > 200).into());
            }
            Value::Object(v)
        })
        .collect();
    Ok(json!({"rows":rows,"total":total}))
}

#[cfg(test)]
mod inventory_tests {
    use super::*;
    #[test]
    fn inventory_projection_preserves_context_without_credentials() {
        let value=project_inventory("connections",&json!({"total":1,"connections":[{"cid":7,"name":"worker","ip":"private","authorized_user":"secret","start":"identity","subscriptions_list":["orders.*"],"in_msgs":22}]})).unwrap();
        assert_eq!(value["rows"][0]["subjects"][0], "orders.*");
        assert!(value["rows"][0].get("ip").is_none());
        assert!(value["rows"][0].get("authorized_user").is_none());
        let value=project_inventory("subscriptions",&json!({"total":1,"subscriptions_list":[{"subject":"orders.*","qgroup":"workers","account":"A","cid":7,"msgs":3}]})).unwrap();
        assert_eq!(value["rows"][0]["qgroup"], "workers");
        assert!(project_inventory("connections", &json!({"error":"denied"})).is_err());
    }
}
fn parse_urls(config: &str) -> Result<Vec<reqwest::Url>, String> {
    let mut urls = vec![];
    for item in config.split(',').map(str::trim).filter(|v| !v.is_empty()) {
        let mut url =
            reqwest::Url::parse(item).map_err(|_| "Invalid NATSUI_MONITOR_URLS endpoint")?;
        if !["http", "https"].contains(&url.scheme())
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || url.path() != "/"
        {
            return Err("Monitoring endpoints must be HTTP(S) origins without credentials, paths, queries or fragments".into());
        }
        url.set_path("/");
        urls.push(url);
    }
    if urls.len() > 32 {
        return Err("At most 32 monitoring endpoints are supported".into());
    }
    Ok(urls)
}
fn project_node(v: &Value, slot: usize, at: u64) -> Value {
    if !v["server_id"].is_string() || !v["start"].is_string() {
        return json!({"slot":slot,"at":at,"status":"unavailable","error":"Invalid NATS server identity"});
    }
    let mut result = serde_json::Map::new();
    for key in [
        "server_id",
        "server_name",
        "start",
        "version",
        "uptime",
        "cpu",
        "mem",
        "cores",
        "connections",
        "total_connections",
        "subscriptions",
        "slow_consumers",
        "in_msgs",
        "out_msgs",
        "in_bytes",
        "out_bytes",
        "config_load_time",
    ] {
        result.insert(key.into(), v[key].clone());
    }
    result.insert("slot".into(), json!(slot));
    result.insert("at".into(), json!(at));
    result.insert("status".into(), json!("complete"));
    result.insert(
        "js_memory".into(),
        v["jetstream"]["stats"]["memory"].clone(),
    );
    result.insert(
        "js_storage".into(),
        v["jetstream"]["stats"]["storage"].clone(),
    );
    result.insert(
        "js_max_storage".into(),
        v["jetstream"]["config"]["max_storage"].clone(),
    );
    result.insert(
        "api_total".into(),
        v["jetstream"]["stats"]["api"]["total"].clone(),
    );
    result.insert(
        "api_errors".into(),
        v["jetstream"]["stats"]["api"]["errors"].clone(),
    );
    for key in [
        "cpu",
        "mem",
        "cores",
        "connections",
        "total_connections",
        "subscriptions",
        "slow_consumers",
        "in_msgs",
        "out_msgs",
        "in_bytes",
        "out_bytes",
        "js_memory",
        "js_storage",
        "js_max_storage",
        "api_total",
        "api_errors",
    ] {
        if !result
            .get(key)
            .and_then(Value::as_f64)
            .is_some_and(|v| v.is_finite() && v >= 0.0)
        {
            result.insert(key.into(), Value::Null);
        }
    }
    Value::Object(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn endpoint_security_is_explicit_and_fails_closed() {
        for config in [
            r#"[]"#,
            r#"[{"url":"https://localhost","password":"secret"}]"#,
            r#"[{"url":"http://localhost","bearer_file":"missing"}]"#,
            r#"[{"url":"https://localhost","cert_file":"missing"}]"#,
            r#"[{"url":"https://localhost","username":"observer"}]"#,
            r#"[{"url":"https://localhost","ca_file":"missing"}]"#,
            r#"[{"url":"https://user:secret@localhost"}]"#,
        ] {
            assert!(super::Monitor::configured(config.as_bytes()).is_err());
        }
        let monitor = super::Monitor::configured(
            br#"[{"url":"https://localhost:8222"},{"url":"http://localhost:8223"}]"#,
        )
        .unwrap();
        assert_eq!(monitor.clients.len(), 2);
        assert_eq!(monitor.credentials.len(), 2);
    }

    use super::*;
    #[tokio::test]
    async fn http_collection_is_bounded_and_failure_is_independent() {
        use axum::{Json, Router, response::Redirect, routing::get};
        let app = Router::new()
            .route(
                "/varz",
                get(|| async {
                    Json(json!({"server_id":"test","start":"start","cpu":2,"mem":123}))
                }),
            )
            .route("/redirect", get(|| async { Redirect::temporary("/varz") }))
            .route("/large", get(|| async { "x".repeat(2_000_001) }));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let monitor = Monitor::new(&format!("http://{address},http://127.0.0.1:0")).unwrap();
        assert!(monitor.get(0, "redirect").await.is_err());
        assert!(monitor.get(0, "large").await.is_err());
        let collector = tokio::spawn(monitor.clone().collect());
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                if monitor.current.read().await["status"] == "partial" {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let state = monitor.current.read().await.clone();
        assert_eq!(state["nodes"][0]["mem"], 123);
        assert_eq!(state["nodes"][1]["status"], "unavailable");
        assert!(state["nodes"][1].get("cpu").is_none());
        collector.abort();
        server.abort();
    }
    #[test]
    fn endpoints_are_explicit_origins() {
        assert!(parse_urls("http://localhost:8222,https://metrics.example").is_ok());
        for bad in [
            "file:///etc/passwd",
            "http://u:p@localhost",
            "http://localhost/connz",
            "http://localhost?secret=x",
        ] {
            assert!(parse_urls(bad).is_err());
        }
    }
    #[test]
    fn projection_omits_sensitive_configuration() {
        let node = project_node(
            &json!({"server_id":"id","start":"t","mem":123,"tls_key":"secret","authorization":{"token":"secret"}}),
            0,
            1,
        );
        assert_eq!(node["mem"], 123);
        assert!(node.get("authorization").is_none());
        assert!(node["cpu"].is_null());
    }
}
