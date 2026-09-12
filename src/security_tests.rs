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
