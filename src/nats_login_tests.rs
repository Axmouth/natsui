use crate::{App, auth, connection, editing, monitoring, nats_login::Policy, store, telemetry};
use axum::{
    Router,
    body::{Body, to_bytes},
    http::{Request, StatusCode},
};
use futures_util::StreamExt;
use serde_json::{Value, json};
use std::{
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::Arc,
    time::Duration,
};
use tokio::sync::RwLock;
use tower::ServiceExt;

struct Broker {
    child: Child,
    directory: PathBuf,
}
fn connection_config(url: &str) -> connection::Config {
    connection::Config {
        url: url.into(),
        credentials: None,
        ca: None,
        certificate: None,
        key: None,
        tls: false,
    }
}
impl Drop for Broker {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        let _ = std::fs::remove_dir_all(&self.directory);
    }
}
fn start(binary: &str, config: &std::path::Path) -> Child {
    let mut command = Command::new(binary);
    command
        .args(["-c", config.to_str().unwrap()])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    command.spawn().unwrap()
}
async fn call(
    router: &Router,
    path: &str,
    method: &str,
    cookie: &str,
    body: Value,
    profile: &str,
) -> (StatusCode, Value, String) {
    let response = router
        .clone()
        .oneshot(
            Request::builder()
                .uri(path)
                .method(method)
                .header("host", "localhost:4321")
                .header("x-natsui-request", "1")
                .header("content-type", "application/json")
                .header("cookie", cookie)
                .header("x-natsui-profile", profile)
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
        .unwrap();
    let status = response.status();
    let cookie = response
        .headers()
        .get("set-cookie")
        .map(|v| v.to_str().unwrap().split(';').next().unwrap().to_owned())
        .unwrap_or_default();
    let bytes = to_bytes(response.into_body(), 24_000_000).await.unwrap();
    let value =
        serde_json::from_slice(&bytes).unwrap_or_else(|_| json!(String::from_utf8_lossy(&bytes)));
    (status, value, cookie)
}
async fn login(router: &Router, username: &str, password: &str) -> String {
    let (status, body, cookie) = call(
        router,
        "/api/auth/nats",
        "POST",
        "",
        json!({"profile":"default","username":username,"password":password}),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{body}");
    assert!(!cookie.is_empty());
    cookie
}
async fn router(url: &str, dir: &std::path::Path, policy: Policy) -> Router {
    let db = store::Database::open(dir.to_str().unwrap()).unwrap();
    let mut snapshot = telemetry::demo("collector");
    snapshot.demo = false;
    let monitor = monitoring::Monitor::new("").unwrap();
    *monitor.current.write().await =
        json!({"status":"complete","nodes":[{"server_name":"collector-node-secret"}]});
    db.sample_resources(&snapshot, &monitor.current.read().await.clone())
        .await
        .unwrap();
    let auth = auth::Auth::with_nats_policy(policy);
    let app = App {
        auth,
        editor: editing::Editor::new(true),
        db,
        current: Arc::new(RwLock::new(snapshot)),
        nats: Arc::new(RwLock::new(None)),
        monitor,
        demo: false,
        prefix: "$JS.API".into(),
        scope: "collector".into(),
        connection: Some(connection_config(url)),
        settings_cache: Arc::new(RwLock::new(store::Settings::default())),
    };
    crate::profiles::router(app, dir.to_str().unwrap())
        .await
        .unwrap()
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER pointing to a local nats-server executable"]
async fn live_nats_login_never_inherits_collector_authority() {
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-user-login-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let config = directory.join("server.json");
    let password = "secret,%40:%ff@/pÃƒÂ¤ss";
    let mut settings = json!({"port":port,"host":"127.0.0.1","jetstream":{"store_dir":directory.join("jetstream")},"accounts":{
        "APP":{"jetstream":"enabled","users":[
            {"user":"collector","password":"collector-secret"},
            {"user":"reader","password":password,"permissions":{"publish":{"allow":["$JS.API.STREAM.LIST","$JS.API.STREAM.INFO.*","$JS.API.STREAM.MSG.GET.ORDERS"]},"subscribe":{"allow":["_INBOX.>"]}}},
            {"user":"payload","password":password,"permissions":{"publish":{"allow":["$JS.API.STREAM.MSG.GET.ORDERS"]},"subscribe":{"allow":["_INBOX.>"]}}},
            {"user":"writer","password":password}
        ]},
        "OTHER":{"jetstream":"enabled","users":[{"user":"other","password":password}]}
    }});
    std::fs::write(&config, settings.to_string()).unwrap();
    drop(reservation);
    let mut broker = Broker {
        child: start(&binary, &config),
        directory: directory.clone(),
    };
    let url = format!("nats://collector:collector-secret@127.0.0.1:{port}");
    let cfg = connection_config(&url);
    let mut admin = None;
    for _ in 0..50 {
        if let Ok(client) = cfg.connect().await {
            admin = Some(client);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let admin = admin.expect("disposable broker ready");
    let js = async_nats::jetstream::new(admin.clone());
    for name in ["ORDERS", "S1", "S2", "S3", "S4", "S5", "S6", "S7"] {
        js.create_stream(async_nats::jetstream::stream::Config {
            name: name.into(),
            subjects: vec![format!("{name}.>")],
            ..Default::default()
        })
        .await
        .unwrap();
    }
    js.publish("ORDERS.one", "stored".into())
        .await
        .unwrap()
        .await
        .unwrap();
    let other = cfg
        .for_user("other", password)
        .unwrap()
        .connect_user()
        .await
        .unwrap();
    async_nats::jetstream::new(other)
        .create_stream(async_nats::jetstream::stream::Config {
            name: "OTHER_ACCOUNT".into(),
            subjects: vec!["other.>".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let private = Policy {
        enabled: true,
        ..Policy::default()
    };
    let dashboard = router(&url, &directory.join("dashboard"), private).await;
    assert_eq!(
        call(
            &dashboard,
            "/api/snapshot",
            "GET",
            "",
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        call(
            &dashboard,
            "/api/auth/nats",
            "POST",
            "",
            json!({"profile":"default","username":"reader","password":"wrong"}),
            "default"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let reader = login(&dashboard, "reader", password).await;
    let payload = login(&dashboard, "payload", password).await;
    let writer = login(&dashboard, "writer", password).await;
    let other = login(&dashboard, "other", password).await;
    assert_eq!(
        call(
            &dashboard,
            "/api/auth/me",
            "GET",
            &reader,
            Value::Null,
            "default"
        )
        .await
        .1["role"],
        "nats"
    );
    for path in [
        "/api/users",
        "/api/managed",
        "/api/nats-users",
        "/api/activity",
        "/api/history",
        "/api/history/window?from=1&to=2",
        "/api/incidents",
        "/api/monitoring/routes",
        "/api/nodes/0/connections",
        "/api/events",
        "/api/future",
    ] {
        assert_eq!(
            call(&dashboard, path, "GET", &reader, Value::Null, "default")
                .await
                .0,
            StatusCode::FORBIDDEN,
            "{path}"
        );
    }
    assert_eq!(
        call(
            &dashboard,
            "/api/settings",
            "PUT",
            &reader,
            json!({}),
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_ne!(
        call(
            &dashboard,
            "/api/snapshot",
            "GET",
            &reader,
            Value::Null,
            "another"
        )
        .await
        .0,
        StatusCode::OK
    );
    let (status, value, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &reader,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(value["snapshot"]["status"], "partial");
    assert_eq!(value["snapshot"]["streams"].as_array().unwrap().len(), 8);
    assert_eq!(value["monitoring"]["status"], "restricted");
    assert!(!value.to_string().contains("collector-node-secret"));
    let (_, value, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &other,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(
        value["snapshot"]["streams"][0]["config"]["name"],
        "OTHER_ACCOUNT"
    );
    assert_eq!(value["snapshot"]["streams"].as_array().unwrap().len(), 1);
    assert_eq!(
        call(
            &dashboard,
            "/api/messages/ORDERS/1",
            "GET",
            &payload,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &dashboard,
            "/api/messages/S1/1",
            "GET",
            &payload,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    let (_, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &reader,
        json!({"action":"delete_stream","stream":"ORDERS"}),
        "default",
    )
    .await;
    assert!(preview["token"].is_string(), "{preview}");
    let approval = json!({"token":preview["token"],"confirmation":preview["confirmation"]});
    assert_ne!(
        call(
            &dashboard,
            "/api/operations/apply",
            "POST",
            &writer,
            approval.clone(),
            "default"
        )
        .await
        .0,
        StatusCode::OK
    );
    assert_eq!(
        call(
            &dashboard,
            "/api/operations/apply",
            "POST",
            &reader,
            approval,
            "default"
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(js.get_stream("ORDERS").await.is_ok());
    let (_, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &writer,
        json!({"action":"create_stream","stream":"CREATED","config":{"subjects":["created.>"]}}),
        "default",
    )
    .await;
    let approval = json!({"token":preview["token"],"confirmation":preview["confirmation"]});
    let (status, result, _) = call(
        &dashboard,
        "/api/operations/apply",
        "POST",
        &writer,
        approval,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(js.get_stream("CREATED").await.is_ok());
    let mut observer = admin.subscribe("forbidden".to_owned()).await.unwrap();
    admin.flush().await.unwrap();
    let (_, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &reader,
        json!({"action":"publish","subject":"forbidden","payload":"denied","mode":"core"}),
        "default",
    )
    .await;
    let (_, result, _) = call(
        &dashboard,
        "/api/operations/apply",
        "POST",
        &reader,
        json!({"token":preview["token"],"confirmation":preview["confirmation"]}),
        "default",
    )
    .await;
    assert_eq!(result["outcome"], "unconfirmed", "{result}");
    assert!(result["accepted_by_server"].is_null());
    assert!(
        tokio::time::timeout(Duration::from_millis(250), observer.next())
            .await
            .is_err()
    );
    let shared = router(
        &url,
        &directory.join("shared"),
        Policy {
            history: true,
            ..private
        },
    )
    .await;
    let shared_cookie = login(&shared, "reader", password).await;
    let (status, value, _) = call(
        &shared,
        "/api/history",
        "GET",
        &shared_cookie,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(!value.as_array().unwrap().is_empty());
    assert!(!value.to_string().contains("collector-node-secret"));
    assert_eq!(
        call(
            &shared,
            "/api/monitoring/routes",
            "GET",
            &shared_cookie,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &dashboard,
            "/api/auth/logout",
            "POST",
            &payload,
            json!({}),
            "default"
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &dashboard,
            "/api/messages/ORDERS/1",
            "GET",
            &payload,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    broker.child.kill().unwrap();
    broker.child.wait().unwrap();
    settings["accounts"]["APP"]["users"][1]["password"] = json!("rotated");
    std::fs::write(&config, settings.to_string()).unwrap();
    broker.child = start(&binary, &config);
    for _ in 0..50 {
        if cfg.connect().await.is_ok() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert_eq!(
        call(
            &dashboard,
            "/api/messages/ORDERS/1",
            "GET",
            &reader,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::BAD_GATEWAY
    );
    assert!(
        call(
            &dashboard,
            "/api/messages/ORDERS/1",
            "GET",
            &writer,
            Value::Null,
            "default"
        )
        .await
        .0
        .is_success()
    );
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER and NATSUI_TLS_FIXTURES"]
async fn password_login_tls_and_unauthenticated_server_boundary() {
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let certs = PathBuf::from(std::env::var("NATSUI_TLS_FIXTURES").unwrap());
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-login-tls-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("nats.json");
    std::fs::write(&path, json!({"host":"127.0.0.1","port":port}).to_string()).unwrap();
    drop(reservation);
    let mut broker = Broker {
        child: start(&binary, &path),
        directory,
    };
    let mut config = connection_config(&format!("nats://collector:secret@127.0.0.1:{port}"));
    let mut ready = false;
    for _ in 0..50 {
        if config.connect().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(ready);
    assert!(
        config
            .for_user("arbitrary", "password")
            .unwrap()
            .connect_user()
            .await
            .is_err()
    );
    broker.child.kill().unwrap();
    broker.child.wait().unwrap();
    let username = "user,%40@alpha";
    let password = "pass,%ff:/@secret";
    std::fs::write(&path,json!({"host":"127.0.0.1","port":port,"authorization":{"users":[{"user":username,"password":password}]},
        "tls":{"cert_file":certs.join("server.pem"),"key_file":certs.join("server.key"),"verify":false}}).to_string()).unwrap();
    broker.child = start(&binary, &path);
    config.ca = Some(certs.join("ca.pem"));
    config.tls = true;
    config.credentials = Some(broker.directory.join("collector-creds-must-not-be-read"));
    let user = config.for_user(username, password).unwrap();
    assert!(user.credentials.is_none());
    let mut ready = false;
    for _ in 0..50 {
        if user.connect_user().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        ready,
        "Password login must preserve literal credentials and TLS trust"
    );
    assert!(
        config
            .for_user(username, "wrong")
            .unwrap()
            .connect_user()
            .await
            .is_err()
    );
    let mut untrusted = user;
    untrusted.ca = None;
    assert!(untrusted.connect_user().await.is_err());
    config.certificate = Some(certs.join("client.pem"));
    config.key = Some(certs.join("client.key"));
    assert!(config.for_user(username, password).is_err());
}
