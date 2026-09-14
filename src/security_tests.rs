use crate::{connection::Config, telemetry};
use serde_json::json;
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};

struct Server {
    child: Child,
    dir: PathBuf,
    binary: String,
    config: PathBuf,
    port: u16,
}
impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}
impl Server {
    fn start(tls: bool) -> Self {
        let binary = std::env::var("NATSUI_TEST_SERVER").expect("NATSUI_TEST_SERVER");
        let socket = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let dir =
            std::env::temp_dir().join(format!("natsui-security-{}-{port}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let certs = std::env::var("NATSUI_TLS_FIXTURES")
            .unwrap_or_default()
            .replace('\\', "/");
        let tls = if tls {
            format!(
                "tls {{cert_file: \"{certs}/server.pem\",key_file: \"{certs}/server.key\",ca_file: \"{certs}/ca.pem\",verify: true}}"
            )
        } else {
            String::new()
        };
        let config = dir.join("server.conf");
        std::fs::write(&config,format!(r#"
listen: "127.0.0.1:{port}"
jetstream {{store_dir:"{}", max_memory_store: 256MB}}
authorization {{users: [
 {{user:admin,password:test-admin}},
 {{user:observer,password:test-reader,permissions:{}}},
 {{user:denied,password:test-denied,permissions:{{publish:{{allow:["nothing"]}},subscribe:{{allow:["_INBOX.>"]}}}}}}
]}}
{tls}
"#,dir.join("store").display().to_string().replace('\\',"/"),include_str!("../deploy/permissions.conf"))).unwrap();
        drop(socket);
        let child = Command::new(&binary)
            .arg("-c")
            .arg(&config)
            .stdout(Stdio::null())
            .stderr(Stdio::from(
                std::fs::File::create(dir.join("server.log")).unwrap(),
            ))
            .spawn()
            .unwrap();
        Self {
            child,
            dir,
            binary,
            config,
            port,
        }
    }
    fn connection(&self, user: &str, password: &str, tls: bool) -> Config {
        let certs = std::env::var("NATSUI_TLS_FIXTURES")
            .map(PathBuf::from)
            .unwrap_or_default();
        Config {
            url: format!("nats://{user}:{password}@127.0.0.1:{}", self.port),
            credentials: None,
            ca: tls.then(|| certs.join("ca.pem")),
            certificate: tls.then(|| certs.join("client.pem")),
            key: tls.then(|| certs.join("client.key")),
            tls,
        }
    }
    async fn client(&self, user: &str, password: &str, tls: bool) -> async_nats::Client {
        for _ in 0..30 {
            if let Ok(c) = self.connection(user, password, tls).connect().await {
                return c;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!(
            "Disposable NATS server failed: {}",
            std::fs::read_to_string(self.dir.join("server.log")).unwrap_or_default()
        )
    }
    fn stop(&mut self) {
        self.child.kill().unwrap();
        self.child.wait().unwrap();
    }
    fn restart(&mut self) {
        self.child = Command::new(&self.binary)
            .arg("-c")
            .arg(&self.config)
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
    }
}
async fn create(client: &async_nats::Client, name: &str) {
    telemetry::request(
        client,
        format!("$JS.API.STREAM.CREATE.{name}"),
        json!({"name":name,"subjects":[format!("{name}.>")],"storage":"file","max_bytes":1048576}),
    )
    .await
    .unwrap();
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER and NATSUI_TLS_FIXTURES from scripts/tls-fixtures.mjs"]
async fn tls_permissions_and_reconnect() {
    let mut server = Server::start(true);
    let admin = server.client("admin", "test-admin", true).await;
    create(&admin, "ORDERS").await;
    admin
        .request("ORDERS.created", "real retained record".into())
        .await
        .unwrap();
    let observer = server.client("observer", "test-reader", true).await;
    assert_eq!(
        telemetry::observe(&observer, "$JS.API", "test")
            .await
            .status,
        "complete"
    );
    assert!(
        crate::messages::lookup(&observer, "$JS.API", "ORDERS", json!({"seq":1}))
            .await
            .unwrap()
            .is_some()
    );
    // Rejected mutations are checked against broker state, not only a timeout.
    assert!(
        telemetry::request(&observer, "$JS.API.STREAM.DELETE.ORDERS".into(), json!({}))
            .await
            .is_err()
    );
    observer
        .publish("ORDERS.created", "forbidden".into())
        .await
        .unwrap();
    observer.flush().await.unwrap();
    let state = telemetry::observe(&admin, "$JS.API", "test").await;
    assert_eq!(state.streams.len(), 1);
    assert_eq!(state.streams[0]["state"]["messages"], 1);
    let denied = server.client("denied", "test-denied", true).await;
    assert_eq!(
        telemetry::observe(&denied, "$JS.API", "test").await.status,
        "unavailable"
    );
    assert!(
        server
            .connection("observer", "wrong", true)
            .connect()
            .await
            .is_err()
    );
    let mut no_trust = server.connection("observer", "test-reader", true);
    no_trust.ca = None;
    assert!(no_trust.connect().await.is_err());
    let mut no_cert = server.connection("observer", "test-reader", true);
    no_cert.certificate = None;
    no_cert.key = None;
    assert!(no_cert.connect().await.is_err());
    server.stop();
    assert_eq!(
        telemetry::observe(&observer, "$JS.API", "test")
            .await
            .status,
        "unavailable"
    );
    server.restart();
    let mut recovered = false;
    for _ in 0..30 {
        if telemetry::observe(&observer, "$JS.API", "test")
            .await
            .status
            == "complete"
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
    assert!(
        recovered,
        "Existing dashboard client must recover after server restart"
    );
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER; creates a disposable large inventory"]
async fn inventory_caps_are_partial() {
    let server = Server::start(false);
    let client = server.client("admin", "test-admin", false).await;
    for i in 0..301 {
        create(&client, &format!("S{i:03}")).await;
    }
    for i in 0..2001 {
        let name = format!("C{i:04}");
        telemetry::request(
            &client,
            format!("$JS.API.CONSUMER.DURABLE.CREATE.S000.{name}"),
            json!({"stream_name":"S000","config":{"durable_name":name,"ack_policy":"explicit"}}),
        )
        .await
        .unwrap();
    }
    let started = std::time::Instant::now();
    let snapshot = telemetry::observe(&client, "$JS.API", "large-test").await;
    assert_eq!(snapshot.status, "partial");
    assert_eq!(snapshot.streams.len(), 300);
    assert_eq!(snapshot.consumers.len(), 2000);
    assert!(snapshot.issues.iter().any(|s| s.contains("300")));
    assert!(
        snapshot
            .issues
            .iter()
            .any(|s| s.contains("2,000") || s.contains("incomplete"))
    );
    println!(
        "Large inventory: 301 streams / 2001 consumers; bounded observation took {:?}",
        started.elapsed()
    );
    let db = crate::store::Database::open(server.dir.join("dashboard").to_str().unwrap()).unwrap();
    db.sample(&snapshot).await.unwrap();
    assert_eq!(
        db.history("large-test", false)
            .await
            .unwrap()
            .as_array()
            .unwrap()
            .len(),
        1
    );
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER"]
async fn reviewed_configuration_edits() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;
    async fn call(
        app: &crate::App,
        method: &str,
        uri: &str,
        body: serde_json::Value,
        header: bool,
    ) -> (u16, serde_json::Value) {
        let mut req = Request::builder()
            .method(method)
            .uri(uri)
            .header("host", "127.0.0.1:4321")
            .header("content-type", "application/json");
        if header {
            req = req.header("x-natsui-request", "1");
        }
        let response = crate::router(app.clone())
            .oneshot(req.body(Body::from(body.to_string())).unwrap())
            .await
            .unwrap();
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes))),
        )
    }
    let server = Server::start(false);
    let admin = server.client("admin", "test-admin", false).await;
    create(&admin, "ORDERS").await;
    admin
        .request("ORDERS.created", "retained".into())
        .await
        .unwrap();
    telemetry::request(&admin,"$JS.API.CONSUMER.CREATE.ORDERS.worker".into(),json!({"stream_name":"ORDERS","config":{"durable_name":"worker","name":"worker","ack_policy":"explicit","max_ack_pending":20,"max_deliver":5},"action":"create"})).await.unwrap();
    let app = crate::App {
        auth: crate::auth::Auth::disabled(),
        editor: crate::editing::Editor::new(true),
        connection: None,
        settings_cache: Arc::new(RwLock::new(crate::store::Settings::default())),
        db: crate::store::Database::open(server.dir.join("dashboard").to_str().unwrap()).unwrap(),
        current: Arc::new(RwLock::new(
            telemetry::observe(&admin, "$JS.API", "test").await,
        )),
        nats: Arc::new(RwLock::new(Some(admin.clone()))),
        monitor: crate::monitoring::Monitor::new("").unwrap(),
        demo: false,
        prefix: "$JS.API".into(),
        scope: "test".into(),
    };
    let (code, loaded) = call(
        &app,
        "GET",
        "/api/config?kind=stream&stream=ORDERS",
        json!({}),
        false,
    )
    .await;
    assert_eq!(code, 200);
    let proposal = json!({"target":{"kind":"stream","stream":"ORDERS"},"revision":loaded["revision"],"changes":{"max_bytes":"2097152"}});
    assert_eq!(
        call(&app, "POST", "/api/config/preview", proposal.clone(), false)
            .await
            .0,
        403
    );
    let mut readonly = app.clone();
    readonly.editor = crate::editing::Editor::default();
    assert_eq!(
        call(
            &readonly,
            "POST",
            "/api/config/preview",
            proposal.clone(),
            true
        )
        .await
        .0,
        403
    );
    let mut simulation = app.clone();
    simulation.demo = true;
    assert_eq!(
        call(
            &simulation,
            "POST",
            "/api/config/preview",
            proposal.clone(),
            true
        )
        .await
        .0,
        403
    );
    let mut invalid = proposal.clone();
    invalid["changes"] = json!({"storage":"memory"});
    assert_eq!(
        call(&app, "POST", "/api/config/preview", invalid, true)
            .await
            .0,
        400
    );
    let (code, preview) = call(&app, "POST", "/api/config/preview", proposal.clone(), true).await;
    assert_eq!(code, 200, "{preview}");
    let approval = json!({"token":preview["token"],"accept_warnings":true});
    let (code, applied) = call(&app, "POST", "/api/config/apply", approval.clone(), true).await;
    assert_eq!(code, 200, "{applied}");
    assert_eq!(applied["verified"], true);
    assert_eq!(applied["audit_saved"], true);
    assert_eq!(
        call(&app, "POST", "/api/config/apply", approval, true)
            .await
            .0,
        409
    );
    assert_eq!(
        call(&app, "POST", "/api/config/preview", proposal, true)
            .await
            .0,
        409
    );
    let (_, loaded) = call(
        &app,
        "GET",
        "/api/config?kind=stream&stream=ORDERS",
        json!({}),
        false,
    )
    .await;
    let proposal = json!({"target":{"kind":"stream","stream":"ORDERS"},"revision":loaded["revision"],"changes":{"max_msgs":"100"}});
    let (_, preview) = call(&app, "POST", "/api/config/preview", proposal, true).await;
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/config/apply",
            json!({"token":preview["token"],"accept_warnings":false}),
            true
        )
        .await
        .0,
        409
    );
    let mut info = telemetry::request(&admin, "$JS.API.STREAM.INFO.ORDERS".into(), json!({}))
        .await
        .unwrap();
    info["config"]["max_msgs"] = json!(200);
    telemetry::request(
        &admin,
        "$JS.API.STREAM.UPDATE.ORDERS".into(),
        info["config"].clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        call(
            &app,
            "POST",
            "/api/config/apply",
            json!({"token":preview["token"],"accept_warnings":true}),
            true
        )
        .await
        .0,
        409
    );
    let (_, loaded) = call(
        &app,
        "GET",
        "/api/config?kind=consumer&stream=ORDERS&consumer=worker",
        json!({}),
        false,
    )
    .await;
    let (_,preview)=call(&app,"POST","/api/config/preview",json!({"target":{"kind":"consumer","stream":"ORDERS","consumer":"worker"},"revision":loaded["revision"],"changes":{"max_ack_pending":"50","backoff":"1, 5"}}),true).await;
    let (code, applied) = call(
        &app,
        "POST",
        "/api/config/apply",
        json!({"token":preview["token"],"accept_warnings":true}),
        true,
    )
    .await;
    assert_eq!(code, 200, "{applied}");
    assert_eq!(applied["verified"], true, "{applied}");
    let info = telemetry::request(
        &admin,
        "$JS.API.CONSUMER.INFO.ORDERS.worker".into(),
        json!({}),
    )
    .await
    .unwrap();
    assert_eq!(info["delivered"]["consumer_seq"], 0);
    assert_eq!(info["num_pending"], 1);
    assert_eq!(info["config"]["max_ack_pending"], 50);
    let observer = server.client("observer", "test-reader", false).await;
    let mut restricted = app.clone();
    restricted.nats = Arc::new(RwLock::new(Some(observer)));
    let (_, loaded) = call(
        &restricted,
        "GET",
        "/api/config?kind=stream&stream=ORDERS",
        json!({}),
        false,
    )
    .await;
    let (_,preview)=call(&restricted,"POST","/api/config/preview",json!({"target":{"kind":"stream","stream":"ORDERS"},"revision":loaded["revision"],"changes":{"max_msgs":"300"}}),true).await;
    let (_, denied) = call(
        &restricted,
        "POST",
        "/api/config/apply",
        json!({"token":preview["token"],"accept_warnings":true}),
        true,
    )
    .await;
    assert_eq!(denied["verified"], false);
    let info = telemetry::request(&admin, "$JS.API.STREAM.INFO.ORDERS".into(), json!({}))
        .await
        .unwrap();
    assert_eq!(info["config"]["max_msgs"], 200);
    assert_eq!(info["state"]["messages"], 1);
    let events = app.db.incident_history("test", false, None).await.unwrap();
    assert!(
        events
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["detail"] == "Natsui configuration edit: applied and verified")
    );
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER and NATSUI_TLS_FIXTURES"]
async fn concurrent_tls_seeds_recover_without_advertised_peers() {
    use futures_util::StreamExt;
    let mut first = Server::start(true);
    let mut second = Server::start(true);
    let first_admin = first.client("admin", "test-admin", true).await;
    let second_admin = second.client("admin", "test-admin", true).await;
    drop(first_admin);
    drop(second_admin);
    first.stop();

    // This endpoint accepts TCP but never completes the NATS handshake.
    let stalled = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let stalled_port = stalled.local_addr().unwrap().port();
    let hold = tokio::spawn(async move {
        let mut sockets = Vec::new();
        while let Ok((socket, _)) = stalled.accept().await {
            sockets.push(socket);
        }
    });
    let mut config = first.connection("observer", "test-reader", true);
    config.url = format!(
        "tls://127.0.0.1:{stalled_port},{},nats://127.0.0.1:{}",
        config.url, second.port
    );
    let connected = tokio::time::timeout(Duration::from_secs(2), config.connect()).await;
    hold.abort();
    let client = connected
        .expect("A stalled seed must not delay the working TLS seed")
        .unwrap();
    assert_eq!(
        telemetry::observe(&client, "$JS.API", "seeds").await.status,
        "complete"
    );
    let original_server = client.server_info().server_id;
    assert!(
        client.server_info().connect_urls.is_empty(),
        "This fixture must exercise configured seeds without discovery"
    );

    // The servers deliberately have no routes. This checks transport fallback, not replication.
    first.restart();
    let admin = first.client("admin", "test-admin", true).await;
    let inbox = client.new_inbox();
    let mut subscription = client.subscribe(inbox.clone()).await.unwrap();
    client.flush().await.unwrap();
    second.stop();
    let mut recovered = false;
    for _ in 0..30 {
        if telemetry::observe(&client, "$JS.API", "seeds").await.status == "complete"
            && client.server_info().server_id != original_server
        {
            recovered = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        recovered,
        "The existing client must retain and reconnect through another configured seed"
    );
    admin.publish(inbox, "after failover".into()).await.unwrap();
    admin.flush().await.unwrap();
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(2), subscription.next())
            .await
            .unwrap()
            .unwrap()
            .payload
            .as_ref(),
        b"after failover"
    );

    let mut no_certificate = config.clone();
    no_certificate.certificate = None;
    no_certificate.key = None;
    assert!(
        no_certificate.connect().await.is_err(),
        "Racing must not bypass client certificate requirements"
    );
    let mut no_trust = config;
    no_trust.ca = None;
    assert!(
        no_trust.connect().await.is_err(),
        "Racing must not bypass server certificate verification"
    );
}

#[tokio::test]
async fn explicit_https_origin_rejects_spoofed_hosts_and_origins() {
    use axum::{
        Router,
        body::Body,
        http::{Request, StatusCode},
        routing::get,
    };
    use tower::ServiceExt;
    let auth = crate::auth::Auth::from_key(&"a".repeat(64)).unwrap();
    auth.configure_origin("https://dashboard.example").unwrap();
    let router = Router::new()
        .fallback(get(|| async { "ok" }).post(|| async { "ok" }))
        .layer(axum::middleware::from_fn_with_state(
            auth,
            crate::network_request,
        ));
    for (path, host, origin, expected) in [
        (
            "/",
            "dashboard.example",
            "https://dashboard.example",
            StatusCode::OK,
        ),
        (
            "/",
            "dashboard.example",
            "http://dashboard.example",
            StatusCode::FORBIDDEN,
        ),
        (
            "/",
            "evil.example",
            "https://dashboard.example",
            StatusCode::FORBIDDEN,
        ),
        (
            "/api/users",
            "localhost:4321",
            "http://localhost:4321",
            StatusCode::FORBIDDEN,
        ),
        (
            "/api/auth/ticket",
            "localhost:4321",
            "http://localhost:4321",
            StatusCode::OK,
        ),
        (
            "/healthz",
            "127.0.0.1:4321",
            "http://127.0.0.1:4321",
            StatusCode::OK,
        ),
    ] {
        let request = Request::builder()
            .uri(path)
            .header("host", host)
            .header("origin", origin)
            .header("x-forwarded-host", "dashboard.example")
            .header("x-forwarded-proto", "https")
            .body(Body::empty())
            .unwrap();
        assert_eq!(
            router.clone().oneshot(request).await.unwrap().status(),
            expected,
            "{path}: {host} {origin}"
        );
    }
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER"]
async fn native_lifecycle_publish_and_bucket_reads() {
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use std::sync::Arc;
    use tokio::sync::RwLock;
    use tower::ServiceExt;
    async fn call(
        app: &crate::App,
        path: &str,
        body: Option<serde_json::Value>,
    ) -> (u16, serde_json::Value) {
        let request = Request::builder()
            .uri(path)
            .method(if body.is_some() { "POST" } else { "GET" })
            .header("host", "localhost:4321")
            .header("content-type", "application/json")
            .header("x-natsui-request", "1")
            .body(Body::from(body.map(|v| v.to_string()).unwrap_or_default()))
            .unwrap();
        let response = crate::router(app.clone()).oneshot(request).await.unwrap();
        let status = response.status().as_u16();
        let bytes = to_bytes(response.into_body(), 2_000_000).await.unwrap();
        (
            status,
            serde_json::from_slice(&bytes)
                .unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes))),
        )
    }
    async fn operation(app: &crate::App, p: serde_json::Value) -> serde_json::Value {
        let (code, preview) = call(app, "/api/operations/preview", Some(p)).await;
        assert_eq!(code, 200, "{preview}");
        let approval = json!({"token":preview["token"],"confirmation":preview["confirmation"]});
        let (code, result) = call(app, "/api/operations/apply", Some(approval.clone())).await;
        assert_eq!(code, 200, "{result}");
        assert_eq!(
            call(app, "/api/operations/apply", Some(approval)).await.0,
            409
        );
        result
    }
    let server = Server::start(false);
    let admin = server.client("admin", "test-admin", false).await;
    let app = crate::App {
        auth: crate::auth::Auth::disabled(),
        editor: crate::editing::Editor::new(true),
        connection: None,
        settings_cache: Arc::new(RwLock::new(crate::store::Settings::default())),
        db: crate::store::Database::open(server.dir.join("dashboard").to_str().unwrap()).unwrap(),
        current: Arc::new(RwLock::new(
            telemetry::observe(&admin, "$JS.API", "test").await,
        )),
        nats: Arc::new(RwLock::new(Some(admin.clone()))),
        monitor: crate::monitoring::Monitor::new("").unwrap(),
        demo: false,
        prefix: "$JS.API".into(),
        scope: "test".into(),
    };
    let created=operation(&app,json!({"action":"create_stream","stream":"CREATED","config":{"subjects":["created.>"],"storage":"memory","retention":"limits","num_replicas":1,"max_msgs":100}})).await;
    assert_eq!(created["verified"], true);
    assert_eq!(call(&app,"/api/operations/preview",Some(json!({"action":"create_stream","stream":"CREATED","config":{"subjects":["created.>"]}}))).await.0,409);
    let published=operation(&app,json!({"action":"publish","mode":"jetstream","stream":"CREATED","subject":"created.one","payload":"stored"})).await;
    assert_eq!(published["storage_verified"], true);
    let core = operation(
        &app,
        json!({"action":"publish","mode":"core","subject":"uncaptured.one","payload":"transient"}),
    )
    .await;
    assert_eq!(core["storage_verified"], false);
    operation(&app,json!({"action":"create_consumer","stream":"CREATED","consumer":"worker","config":{"deliver_policy":"all","filter_subject":"created.>"}})).await;
    let before = telemetry::request(
        &admin,
        "$JS.API.CONSUMER.INFO.CREATED.worker".into(),
        json!({}),
    )
    .await
    .unwrap();
    let js = async_nats::jetstream::new(admin.clone());
    let kv = js
        .create_key_value(async_nats::jetstream::kv::Config {
            bucket: "config".into(),
            history: 3,
            ..Default::default()
        })
        .await
        .unwrap();
    kv.put("active", "value".into()).await.unwrap();
    kv.put("removed", "old".into()).await.unwrap();
    kv.delete("removed").await.unwrap();
    let objects = js
        .create_object_store(async_nats::jetstream::object_store::Config {
            bucket: "reports".into(),
            ..Default::default()
        })
        .await
        .unwrap();
    objects
        .put("test.txt", &mut &b"object content"[..])
        .await
        .unwrap();
    *app.current.write().await = telemetry::observe(&admin, "$JS.API", "test").await;
    let (code, buckets) = call(&app, "/api/buckets", None).await;
    assert_eq!(code, 200);
    assert_eq!(buckets["buckets"].as_array().unwrap().len(), 2);
    let (code, keys) = call(&app, "/api/buckets/kv/config", None).await;
    assert_eq!(code, 200, "{keys}");
    assert_eq!(keys["rows"].as_array().unwrap().len(), 2);
    let (code, entry) = call(&app, "/api/buckets/kv/config/entry?key=active", None).await;
    assert_eq!(code, 200, "{entry}");
    assert_eq!(entry["text"], "value");
    assert_eq!(entry["present"], true);
    let (_, deleted) = call(&app, "/api/buckets/kv/config/entry?key=removed", None).await;
    assert_eq!(deleted["operation"], "DEL");
    assert_eq!(deleted["present"], false);
    let (code, keys) = call(&app, "/api/buckets/object/reports", None).await;
    assert_eq!(code, 200, "{keys}");
    assert_eq!(keys["rows"][0]["name"], "test.txt");
    let key = keys["rows"][0]["key"].as_str().unwrap();
    let (code, object) = call(
        &app,
        &format!("/api/buckets/object/reports/entry?key={key}"),
        None,
    )
    .await;
    assert_eq!(code, 200, "{object}");
    assert_eq!(object["size"], 14);
    let after = telemetry::request(
        &admin,
        "$JS.API.CONSUMER.INFO.CREATED.worker".into(),
        json!({}),
    )
    .await
    .unwrap();
    for field in ["delivered", "ack_floor", "num_pending", "num_ack_pending"] {
        assert_eq!(before[field], after[field]);
    }
    let (_, preview) = call(
        &app,
        "/api/operations/preview",
        Some(json!({"action":"delete_stream","stream":"CREATED"})),
    )
    .await;
    let mut info = telemetry::request(&admin, "$JS.API.STREAM.INFO.CREATED".into(), json!({}))
        .await
        .unwrap();
    info["config"]["max_msgs"] = json!(50);
    telemetry::request(
        &admin,
        "$JS.API.STREAM.UPDATE.CREATED".into(),
        info["config"].clone(),
    )
    .await
    .unwrap();
    assert_eq!(
        call(
            &app,
            "/api/operations/apply",
            Some(json!({"token":preview["token"],"confirmation":preview["confirmation"]}))
        )
        .await
        .0,
        409
    );
    operation(
        &app,
        json!({"action":"delete_consumer","stream":"CREATED","consumer":"worker"}),
    )
    .await;
    operation(&app, json!({"action":"delete_stream","stream":"CREATED"})).await;
}
