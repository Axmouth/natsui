#[path = "session_tools_tests.rs"]
mod session_tools_tests;

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
        session_tls: None,
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
    router_config(connection_config(url), dir, policy).await
}
async fn router_config(
    config: connection::Config,
    dir: &std::path::Path,
    policy: Policy,
) -> Router {
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
        accounts: Default::default(),
        connection: Some(config),
        settings_cache: Arc::new(RwLock::new(store::Settings::default())),
    };
    crate::profiles::router(app, Some(dir.to_str().unwrap()))
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
    let session_dashboard = router(
        &format!("nats://127.0.0.1:{port}"),
        &directory.join("sessions"),
        Policy {
            session_only: true,
            ..private
        },
    )
    .await;
    let first = login(&session_dashboard, "writer", password).await;
    let second = login(&session_dashboard, "writer", password).await;
    let (_, before, _) = call(
        &session_dashboard,
        "/api/history",
        "GET",
        &first,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(
        before,
        json!([]),
        "Collector samples must never enter session history"
    );
    let (status, snapshot, _) = call(
        &session_dashboard,
        "/api/snapshot",
        "GET",
        &first,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{snapshot}");
    assert_eq!(snapshot["dashboard"]["session_history"], true);
    let (_, recorded, _) = call(
        &session_dashboard,
        "/api/history",
        "GET",
        &first,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(recorded.as_array().unwrap().len(), 1);
    assert!(!recorded.to_string().contains("collector-node-secret"));
    let (_, isolated, _) = call(
        &session_dashboard,
        "/api/history",
        "GET",
        &second,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(
        isolated,
        json!([]),
        "Even identical usernames have independent histories"
    );
    let from = telemetry::now().saturating_sub(60);
    for path in [
        format!("/api/history/window?from={from}&to={}", from + 120),
        format!("/api/history/backlog?from={from}&to={}", from + 120),
    ] {
        assert_eq!(
            call(
                &session_dashboard,
                &path,
                "GET",
                &first,
                Value::Null,
                "default"
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    assert_eq!(
        call(
            &session_dashboard,
            "/api/monitoring/routes",
            "GET",
            &first,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    assert_eq!(
        call(
            &session_dashboard,
            "/api/auth/logout",
            "POST",
            &first,
            json!({}),
            "default"
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    assert_eq!(
        call(
            &session_dashboard,
            "/api/history",
            "GET",
            &first,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::UNAUTHORIZED
    );
    let fresh = login(&session_dashboard, "writer", password).await;
    assert_eq!(
        call(
            &session_dashboard,
            "/api/history",
            "GET",
            &fresh,
            Value::Null,
            "default"
        )
        .await
        .1,
        json!([])
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

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER and NATSUI_TLS_FIXTURES"]
async fn certificate_login_maps_identity_and_keeps_key_in_memory() {
    use base64::Engine;
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let certs = PathBuf::from(std::env::var("NATSUI_TLS_FIXTURES").unwrap());
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-cert-login-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("nats.json");
    std::fs::write(&path, json!({"host":"127.0.0.1","port":port,
        "jetstream":{"store_dir":directory.join("jetstream")},
        "authorization":{"users":[{"user":"CN=natsui-test","permissions":{"publish":{"allow":["$JS.API.STREAM.LIST"]},"subscribe":{"allow":["_INBOX.>"]}}}]},
        "tls":{"cert_file":certs.join("server.pem"),"key_file":certs.join("server.key"),"ca_file":certs.join("ca.pem"),"verify_and_map":true}
    }).to_string()).unwrap();
    drop(reservation);
    let _broker = Broker {
        child: start(&binary, &path),
        directory: directory.clone(),
    };
    let mut config = connection_config(&format!("tls://127.0.0.1:{port}"));
    // The extension does not determine encoding. Deployment CA files can use .crt.
    std::fs::copy(certs.join("ca.pem"), directory.join("ca.crt")).unwrap();
    config.ca = Some(directory.join("ca.crt"));
    let certificate = [
        b"\xef\xbb\xbf\n  ".as_slice(),
        &std::fs::read(certs.join("client.pem")).unwrap(),
    ]
    .concat();
    let key = std::fs::read(certs.join("client.key")).unwrap();
    let (identity, fingerprint) = config.for_certificate(&certificate, &key).unwrap();
    assert!(
        identity.credentials.is_none() && identity.certificate.is_none() && identity.key.is_none()
    );
    assert!(fingerprint.starts_with("certificate:"));
    assert!(
        config
            .for_certificate(
                &certificate,
                &std::fs::read(certs.join("server.key")).unwrap()
            )
            .is_err()
    );
    let mut ready = false;
    for _ in 0..50 {
        if identity.connect_user().await.is_ok() {
            ready = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    assert!(
        ready,
        "Mapped certificate must authenticate without password"
    );
    let policy = Policy {
        enabled: true,
        certificates: true,
        session_only: true,
        ..Default::default()
    };
    let dashboard = router_config(config.clone(), &directory.join("dashboard"), policy).await;
    let body = json!({"profile":"default", "certificate":base64::engine::general_purpose::STANDARD.encode(&certificate), "private_key":std::str::from_utf8(&key).unwrap()});
    let (status, result, cookie) = call(
        &dashboard,
        "/api/auth/nats",
        "POST",
        "",
        body.clone(),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::NO_CONTENT, "{result}");
    let (status, details, _) = call(
        &dashboard,
        "/api/session",
        "GET",
        &cookie,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert!(
        details["certificate"]["subject"]
            .as_str()
            .unwrap()
            .contains("CN=natsui-test")
    );
    assert!(details["certificate"]["expires_at"].as_u64().unwrap() > telemetry::now());
    assert!(!details.to_string().contains("PRIVATE KEY"));
    let (status, snapshot, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &cookie,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(snapshot["snapshot"]["status"], "complete", "{snapshot}");
    assert_eq!(
        call(
            &dashboard,
            "/api/users",
            "GET",
            &cookie,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    let (status, denied, _) = call(
        &dashboard,
        "/api/messages/SECRET/1",
        "GET",
        &cookie,
        Value::Null,
        "default",
    )
    .await;
    assert!(
        !status.is_success(),
        "Mapped user has no message permission: {denied}"
    );
    // DER certificates use the same identity and support binary .crt exports.
    use async_nats::rustls::pki_types::{CertificateDer, pem::PemObject};
    let der =
        CertificateDer::from_pem_slice(&std::fs::read(certs.join("client.pem")).unwrap()).unwrap();
    let mut der_body = body.clone();
    der_body["certificate"] = json!(base64::engine::general_purpose::STANDARD.encode(der.as_ref()));
    assert_eq!(
        call(
            &dashboard,
            "/api/auth/nats",
            "POST",
            "",
            der_body,
            "default"
        )
        .await
        .0,
        StatusCode::NO_CONTENT
    );
    // Mounted CA and client identities support binary DER independently of the file suffix.
    let ca_der =
        CertificateDer::from_pem_slice(&std::fs::read(certs.join("ca.pem")).unwrap()).unwrap();
    std::fs::write(directory.join("ca-der.crt"), ca_der.as_ref()).unwrap();
    std::fs::write(directory.join("client-der.crt"), der.as_ref()).unwrap();
    let mut mounted = config.clone();
    mounted.ca = Some(directory.join("ca-der.crt"));
    mounted.certificate = Some(directory.join("client-der.crt"));
    mounted.key = Some(certs.join("client.key"));
    assert!(mounted.connect().await.is_ok());
    let chain = [
        certificate.clone(),
        std::fs::read(certs.join("ca.pem")).unwrap(),
    ]
    .concat();
    let (chain_identity, chain_fingerprint) = mounted.for_certificate(&chain, &key).unwrap();
    assert_eq!(chain_fingerprint, fingerprint);
    assert!(chain_identity.connect_user().await.is_ok());
    let (_, der_fingerprint) = mounted.for_certificate(der.as_ref(), &key).unwrap();
    assert_eq!(der_fingerprint, fingerprint);
    let mut mixed = body.clone();
    mixed["username"] = json!("collector");
    assert_eq!(
        call(&dashboard, "/api/auth/nats", "POST", "", mixed, "default")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let mut bad = body.clone();
    bad["private_key"] = json!("not a key");
    assert_eq!(
        call(&dashboard, "/api/auth/nats", "POST", "", bad, "default")
            .await
            .0,
        StatusCode::BAD_REQUEST
    );
    let disabled = router_config(
        config.clone(),
        &directory.join("disabled"),
        Policy {
            certificates: false,
            ..policy
        },
    )
    .await;
    assert_eq!(
        call(&disabled, "/api/auth/nats", "POST", "", body, "default")
            .await
            .0,
        StatusCode::FORBIDDEN
    );
    let mut untrusted = config.clone();
    untrusted.ca = Some(certs.join("server.pem"));
    assert!(
        untrusted
            .for_certificate(&certificate, &key)
            .unwrap()
            .0
            .connect_user()
            .await
            .is_err()
    );
    config.credentials = Some("collector-creds-must-not-be-read".into());
    assert!(
        config
            .for_certificate(&certificate, &key)
            .unwrap()
            .0
            .connect_user()
            .await
            .is_ok()
    );
}
