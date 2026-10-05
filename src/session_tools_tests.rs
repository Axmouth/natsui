use super::*;

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER"]
async fn session_tools_use_only_native_user_authority() {
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-tools-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("nats.json");
    std::fs::write(&path,json!({"host":"127.0.0.1","port":port,"server_name":"session-node","system_account":"SYS",
        "jetstream":{"store_dir":directory.join("js")},"accounts":{
            "SYS":{"users":[{"user":"system","password":"secret"}]},
            "APP":{"jetstream":"enabled","users":[{"user":"writer","password":"secret"},
                {"user":"reader","password":"secret","permissions":{"publish":{"allow":["$JS.API.STREAM.LIST","$JS.API.CONSUMER.LIST.*","$JS.API.CONSUMER.INFO.*.*"]},"subscribe":{"allow":["_INBOX.>"]}}}]}
        }}).to_string()).unwrap();
    drop(reservation);
    let mut broker = Broker {
        child: start(&binary, &path),
        directory: directory.clone(),
    };
    let cfg = connection_config(&format!("nats://writer:secret@127.0.0.1:{port}"));
    let mut ready = None;
    for _ in 0..50 {
        if let Ok(client) = cfg.connect().await {
            ready = Some(client);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let client = ready.expect("test broker startup");
    let js = async_nats::jetstream::new(client.clone());
    let stream = js
        .create_stream(async_nats::jetstream::stream::Config {
            name: "JOBS".into(),
            subjects: vec!["jobs".into()],
            ..Default::default()
        })
        .await
        .unwrap();
    let consumer = stream
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("worker".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    js.publish("jobs", "example".into())
        .await
        .unwrap()
        .await
        .unwrap();
    let dashboard = router(
        &format!("nats://127.0.0.1:{port}"),
        &directory.join("dashboard"),
        Policy {
            enabled: true,
            session_only: true,
            ..Default::default()
        },
    )
    .await;
    let writer = login(&dashboard, "writer", "secret").await;
    let reader = login(&dashboard, "reader", "secret").await;
    let second = login(&dashboard, "writer", "secret").await;
    let system = login(&dashboard, "system", "secret").await;
    let until = (time::OffsetDateTime::now_utc() + time::Duration::hours(1))
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap();
    let pause = json!({"action":"pause_consumer","stream":"JOBS","consumer":"worker","config":{"pause_until":until}});
    let approval =
        |preview: &Value| json!({"token":preview["token"],"confirmation":preview["confirmation"]});
    let (status, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &reader,
        pause.clone(),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(
        !call(
            &dashboard,
            "/api/operations/apply",
            "POST",
            &reader,
            approval(&preview),
            "default"
        )
        .await
        .0
        .is_success()
    );
    assert!(
        consumer
            .clone()
            .info()
            .await
            .unwrap()
            .config
            .pause_until
            .is_none()
    );
    let (status, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &writer,
        pause.clone(),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    assert!(
        !call(
            &dashboard,
            "/api/operations/apply",
            "POST",
            &second,
            approval(&preview),
            "default"
        )
        .await
        .0
        .is_success()
    );
    let (status, result, _) = call(
        &dashboard,
        "/api/operations/apply",
        "POST",
        &writer,
        approval(&preview),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["verified"], true, "{result}");
    let (_, stale, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &writer,
        pause,
        "default",
    )
    .await;
    let (status, preview, _) = call(
        &dashboard,
        "/api/operations/preview",
        "POST",
        &writer,
        json!({"action":"resume_consumer","stream":"JOBS","consumer":"worker"}),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{preview}");
    let (status, result, _) = call(
        &dashboard,
        "/api/operations/apply",
        "POST",
        &writer,
        approval(&preview),
        "default",
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["verified"], true, "{result}");
    assert_eq!(
        call(
            &dashboard,
            "/api/operations/apply",
            "POST",
            &writer,
            approval(&stale),
            "default"
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let (_, diary, _) = call(
        &dashboard,
        "/api/activity",
        "GET",
        &writer,
        Value::Null,
        "default",
    )
    .await;
    assert!(diary.to_string().contains("pause_consumer"), "{diary}");
    let (_, diary, _) = call(
        &dashboard,
        "/api/activity",
        "GET",
        &second,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(diary, json!([]), "{diary}");

    assert_eq!(
        call(
            &dashboard,
            "/api/monitoring/connections",
            "GET",
            &system,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for cookie in [&system, &reader] {
        assert_eq!(
            call(
                &dashboard,
                "/api/session/monitoring",
                "POST",
                cookie,
                json!({"enabled":true}),
                "default"
            )
            .await
            .0,
            StatusCode::OK
        );
    }
    let (_, snapshot, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &system,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(
        snapshot["monitoring"]["source"], "session_nats",
        "{snapshot}"
    );
    assert_eq!(snapshot["monitoring"]["status"], "partial");
    assert_eq!(
        snapshot["monitoring"]["nodes"][0]["server_name"], "session-node",
        "{snapshot}"
    );
    for kind in ["connections", "subscriptions", "routes"] {
        let (status, inventory, _) = call(
            &dashboard,
            &format!("/api/monitoring/{kind}"),
            "GET",
            &system,
            Value::Null,
            "default",
        )
        .await;
        assert_eq!(status, StatusCode::OK, "{kind}: {inventory}");
        assert_eq!(inventory["nodes"][0]["slot"], 0);
        assert!(!inventory.to_string().contains("secret"));
    }
    assert_eq!(
        call(
            &dashboard,
            "/api/nodes/0/connections",
            "GET",
            &system,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::OK
    );
    let (_, snapshot, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &reader,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(
        snapshot["monitoring"]["status"], "unavailable",
        "{snapshot}"
    );
    assert!(!snapshot.to_string().contains("collector-node-secret"));
    assert!(
        !call(
            &dashboard,
            "/api/monitoring/connections",
            "GET",
            &reader,
            Value::Null,
            "default"
        )
        .await
        .0
        .is_success()
    );
    let (_, history, _) = call(
        &dashboard,
        "/api/history",
        "GET",
        &reader,
        Value::Null,
        "default",
    )
    .await;
    assert!(!history.to_string().contains("session-node"));

    // The fixture application terminates its own delivery. Dashboard inspection never acknowledges it.
    let watching = {
        let dashboard = dashboard.clone();
        let writer = writer.clone();
        tokio::spawn(async move {
            call(
                &dashboard,
                "/api/watch?stream=JOBS",
                "GET",
                &writer,
                Value::Null,
                "default",
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert_eq!(
        call(
            &dashboard,
            "/api/watch?stream=JOBS",
            "GET",
            &writer,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::CONFLICT
    );
    let mut messages = consumer
        .fetch()
        .max_messages(1)
        .expires(Duration::from_secs(2))
        .messages()
        .await
        .unwrap();
    messages
        .next()
        .await
        .unwrap()
        .unwrap()
        .ack_with(async_nats::jetstream::AckKind::Term)
        .await
        .unwrap();
    client.flush().await.unwrap();
    let (status, result, _) = watching.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert!(
        result["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(
                |e| e["detail"] == "io.nats.jetstream.advisory.v1.terminated"
                    && e["stream_seq"] == 1
            ),
        "{result}"
    );
    let (_, events, _) = call(
        &dashboard,
        "/api/incidents",
        "GET",
        &writer,
        Value::Null,
        "default",
    )
    .await;
    assert!(events.to_string().contains("advisory-coverage"), "{events}");
    let (_, events, _) = call(
        &dashboard,
        "/api/incidents",
        "GET",
        &second,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(events, json!([]));
    let (_, result, _) = call(
        &dashboard,
        "/api/watch?stream=JOBS",
        "GET",
        &reader,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(result["events"], json!([]));
    assert!(
        result["detail"]
            .as_str()
            .unwrap()
            .contains("does not confirm")
    );
    let watching = {
        let dashboard = dashboard.clone();
        let writer = writer.clone();
        tokio::spawn(async move {
            call(
                &dashboard,
                "/api/watch?stream=JOBS",
                "GET",
                &writer,
                Value::Null,
                "default",
            )
            .await
        })
    };
    tokio::time::sleep(Duration::from_millis(500)).await;
    broker.child.kill().unwrap();
    broker.child.wait().unwrap();
    let (status, result, _) = watching.await.unwrap();
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(
        result["gap"], true,
        "A broken connection is not a complete window: {result}"
    );
    assert_eq!(result["end_reason"], "connection lost before the deadline");
}
