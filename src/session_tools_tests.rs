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
            &reader,
            Value::Null,
            "default"
        )
        .await
        .0,
        StatusCode::FORBIDDEN
    );
    for (cookie, enabled) in [(&system, true), (&reader, false)] {
        let (_, session, _) = call(
            &dashboard,
            "/api/session",
            "GET",
            cookie,
            Value::Null,
            "default",
        )
        .await;
        assert_eq!(session["monitoring"], enabled, "{session}");
    }
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

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER"]
async fn granted_access_scopes_the_snapshot_without_denied_requests() {
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-scope-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("nats.json");
    let own = |stream: &str| {
        json!([
            "$SYS.REQ.USER.INFO",
            format!("$JS.API.STREAM.INFO.{stream}"),
            format!("$JS.API.CONSUMER.LIST.{stream}")
        ])
    };
    std::fs::write(&path,json!({"host":"127.0.0.1","port":port,"server_name":"scope-node","system_account":"SYS",
        "jetstream":{"store_dir":directory.join("js")},"accounts":{
            "SYS":{"users":[{"user":"system","password":"secret"}]},
            "APP":{"jetstream":"enabled","users":[{"user":"admin","password":"secret"},
                {"user":"named","password":"secret","permissions":{"publish":{"allow":own("JOBS"),"deny":["$JS.API.STREAM.LIST","$JS.API.STREAM.NAMES"]},"subscribe":{"allow":["_INBOX.>"]}}},
                {"user":"listing","password":"secret","permissions":{"publish":{"allow":["$SYS.REQ.USER.INFO","$JS.API.STREAM.LIST","$JS.API.STREAM.INFO.JOBS"]},"subscribe":{"allow":["_INBOX.>"]}}}]}
        }}).to_string()).unwrap();
    drop(reservation);
    let _broker = Broker {
        child: start(&binary, &path),
        directory: directory.clone(),
    };
    let cfg = connection_config(&format!("nats://admin:secret@127.0.0.1:{port}"));
    let mut ready = None;
    for _ in 0..50 {
        if let Ok(client) = cfg.connect().await {
            ready = Some(client);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let js = async_nats::jetstream::new(ready.expect("test broker startup"));
    for (name, subject) in [("JOBS", "jobs"), ("OTHER", "other")] {
        js.create_stream(async_nats::jetstream::stream::Config {
            name: name.into(),
            subjects: vec![subject.into()],
            ..Default::default()
        })
        .await
        .unwrap()
        .create_consumer(async_nats::jetstream::consumer::pull::Config {
            durable_name: Some("worker".into()),
            ..Default::default()
        })
        .await
        .unwrap();
    }
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
    let snapshot = |user: &'static str| {
        let dashboard = dashboard.clone();
        async move {
            let cookie = login(&dashboard, user, "secret").await;
            call(
                &dashboard,
                "/api/snapshot",
                "GET",
                &cookie,
                Value::Null,
                "default",
            )
            .await
            .1["snapshot"]
                .clone()
        }
    };
    let names = |snapshot: &Value| -> Vec<String> {
        snapshot["streams"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["config"]["name"].as_str().unwrap().to_owned())
            .collect()
    };

    let named = snapshot("named").await;
    assert_eq!(named["status"], "complete", "{named}");
    assert_eq!(names(&named), ["JOBS"]);
    assert_eq!(named["consumers"].as_array().unwrap().len(), 1);
    assert_eq!(named["access"]["user"], "named");
    assert_eq!(named["access"]["restricted"], true);
    assert_eq!(named["access"]["lists_streams"], false);

    let listing = snapshot("listing").await;
    assert_eq!(listing["status"], "complete", "{listing}");
    assert_eq!(names(&listing), ["JOBS"]);
    assert_eq!(listing["streams"][0]["natsui_consumers"], "not_granted");
    assert!(listing["consumers"].as_array().unwrap().is_empty());

    let admin = snapshot("admin").await;
    assert_eq!(admin["status"], "complete", "{admin}");
    assert_eq!(names(&admin).len(), 2);
    assert_eq!(admin["access"]["restricted"], false);
}

#[tokio::test]
#[ignore = "Requires NATSUI_TEST_SERVER"]
async fn record_management_follows_grants() {
    let binary = std::env::var("NATSUI_TEST_SERVER").unwrap();
    let reservation = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = reservation.local_addr().unwrap().port();
    let directory =
        std::env::temp_dir().join(format!("natsui-records-{}-{port}", std::process::id()));
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("nats.json");
    std::fs::write(&path,json!({"host":"127.0.0.1","port":port,"server_name":"records-node","system_account":"SYS",
        "jetstream":{"store_dir":directory.join("js")},"accounts":{
            "SYS":{"users":[{"user":"system","password":"secret"}]},
            "APP":{"jetstream":"enabled","users":[{"user":"admin","password":"secret"},
                {"user":"reader","password":"secret","permissions":{"publish":{"allow":["$SYS.REQ.USER.INFO","$JS.API.STREAM.INFO.JOBS","$JS.API.STREAM.MSG.GET.JOBS"]},"subscribe":{"allow":["_INBOX.>"]}}}]}
        }}).to_string()).unwrap();
    drop(reservation);
    let _broker = Broker {
        child: start(&binary, &path),
        directory: directory.clone(),
    };
    let cfg = connection_config(&format!("nats://admin:secret@127.0.0.1:{port}"));
    let mut ready = None;
    for _ in 0..50 {
        if let Ok(client) = cfg.connect().await {
            ready = Some(client);
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let js = async_nats::jetstream::new(ready.expect("test broker startup"));
    js.create_stream(async_nats::jetstream::stream::Config {
        name: "JOBS".into(),
        subjects: vec!["jobs.>".into()],
        ..Default::default()
    })
    .await
    .unwrap();
    for (id, subject) in [("1", "jobs.a"), ("2", "jobs.b"), ("3", "jobs.a")] {
        let mut headers = async_nats::HeaderMap::new();
        headers.insert("Nats-Msg-Id", id);
        headers.insert("Trace", "t1");
        js.publish_with_headers(subject, headers, "work".into())
            .await
            .unwrap()
            .await
            .unwrap();
    }
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
    let operate = |cookie: String, proposal: Value| {
        let dashboard = dashboard.clone();
        async move {
            let (status, preview, _) = call(
                &dashboard,
                "/api/operations/preview",
                "POST",
                &cookie,
                proposal,
                "default",
            )
            .await;
            assert_eq!(status, StatusCode::OK, "{preview}");
            call(
                &dashboard,
                "/api/operations/apply",
                "POST",
                &cookie,
                json!({"token":preview["token"],"confirmation":preview["confirmation"]}),
                "default",
            )
            .await
        }
    };
    let admin = login(&dashboard, "admin", "secret").await;
    let reader = login(&dashboard, "reader", "secret").await;

    let (_, snapshot, _) = call(
        &dashboard,
        "/api/snapshot",
        "GET",
        &reader,
        Value::Null,
        "default",
    )
    .await;
    assert_eq!(snapshot["dashboard"]["login_user"], "reader");
    let caps = &snapshot["snapshot"]["capabilities"];
    assert_eq!(caps["known"], true, "{snapshot}");
    let purge = caps["streams"][0]["capabilities"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["id"] == "purge")
        .unwrap();
    assert_eq!(purge["granted"], false);
    assert_eq!(purge["needs"], json!(["$JS.API.STREAM.PURGE.JOBS"]));

    let (status, result, _) = operate(
        admin.clone(),
        json!({"action":"republish_message","stream":"JOBS","sequence":1}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["sequence"], 4);
    assert_eq!(result["subject"], "jobs.a");
    let copy = js
        .get_stream("JOBS")
        .await
        .unwrap()
        .get_raw_message(4)
        .await
        .unwrap();
    let headers = copy.headers;
    assert!(headers.get("Nats-Msg-Id").is_none());
    assert_eq!(headers.get("Trace").unwrap().as_str(), "t1");
    assert_eq!(
        headers.get("Natsui-Republished-From").unwrap().as_str(),
        "JOBS#1"
    );

    let (status, result, _) = operate(
        admin.clone(),
        json!({"action":"delete_message","stream":"JOBS","sequence":2}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["verified"], true);

    let (status, result, _) = operate(
        admin.clone(),
        json!({"action":"purge_stream","stream":"JOBS","config":{"filter":"jobs.a"}}),
    )
    .await;
    assert_eq!(status, StatusCode::OK, "{result}");
    assert_eq!(result["purged"], 3);

    let (status, _, _) = operate(reader, json!({"action":"purge_stream","stream":"JOBS"})).await;
    assert_ne!(status, StatusCode::OK);
    assert_eq!(
        js.get_stream("JOBS")
            .await
            .unwrap()
            .info()
            .await
            .unwrap()
            .state
            .messages,
        0
    );
}
