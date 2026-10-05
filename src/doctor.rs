use crate::{auth, connection, deployment, nats_login, telemetry};
use serde::Serialize;
use serde_json::{Value, json};
use std::{
    io::Read,
    path::{Path, PathBuf},
    time::Duration,
};

const HELP: &str = "Usage: natsui doctor [--json] [--require-session-only]\n       natsui doctor --probe [--profile ID] (--username NAME --password-file PATH | --certificate-file PATH --key-file PATH) [--stream NAME] [--consumer NAME] [--json]\n\nLocal checks reuse startup validators. --probe uses only the supplied identity for bounded, read-only NATS requests. No configuration, users, messages or database files are changed. Exit: 0 no failed checks, 1 failed check, 2 invalid arguments. Unknown checks do not imply readiness.";

#[derive(Default)]
struct Options {
    json: bool,
    probe: bool,
    require_session: bool,
    profile: String,
    username: Option<String>,
    password: Option<PathBuf>,
    certificate: Option<PathBuf>,
    key: Option<PathBuf>,
    stream: Option<String>,
    consumer: Option<String>,
}
impl Options {
    fn parse(args: &[String]) -> Result<Self, ()> {
        let mut options = Self {
            profile: "default".into(),
            ..Self::default()
        };
        let mut seen = std::collections::BTreeSet::new();
        let mut args = args.iter();
        while let Some(arg) = args.next() {
            if !seen.insert(arg) {
                return Err(());
            }
            match arg.as_str() {
                "--json" => options.json = true,
                "--probe" => options.probe = true,
                "--require-session-only" => options.require_session = true,
                "--profile" => options.profile = args.next().ok_or(())?.clone(),
                "--username" => options.username = Some(args.next().ok_or(())?.clone()),
                "--password-file" => options.password = Some(args.next().ok_or(())?.into()),
                "--certificate-file" => options.certificate = Some(args.next().ok_or(())?.into()),
                "--key-file" => options.key = Some(args.next().ok_or(())?.into()),
                "--stream" => options.stream = Some(args.next().ok_or(())?.clone()),
                "--consumer" => options.consumer = Some(args.next().ok_or(())?.clone()),
                _ => return Err(()),
            }
        }
        let password = options.username.is_some() && options.password.is_some();
        let certificate = options.certificate.is_some() && options.key.is_some();
        if options.username.is_some() != options.password.is_some()
            || options.certificate.is_some() != options.key.is_some()
            || options.probe != (password || certificate)
            || (password && certificate)
            || options
                .username
                .as_ref()
                .is_some_and(|v| v.is_empty() || v.len() > 1024)
            || options.consumer.is_some() && options.stream.is_none()
            || options
                .stream
                .iter()
                .chain(options.consumer.iter())
                .any(|v| !telemetry::valid_token(v))
            || (!options.probe && (options.stream.is_some() || options.profile != "default"))
        {
            return Err(());
        }
        Ok(options)
    }
}

#[derive(Serialize)]
struct Check {
    id: String,
    status: &'static str,
    detail: String,
    guidance: String,
}
#[derive(Serialize)]
struct Report {
    format: &'static str,
    version: &'static str,
    mode: &'static str,
    checks: Vec<Check>,
}
impl Report {
    fn add(
        &mut self,
        id: impl Into<String>,
        status: &'static str,
        detail: impl Into<String>,
        guidance: impl Into<String>,
    ) {
        self.checks.push(Check {
            id: id.into(),
            status,
            detail: detail.into(),
            guidance: guidance.into(),
        });
    }
    fn validation<T, E>(
        &mut self,
        id: &str,
        result: Result<T, E>,
        good: &str,
        bad: &str,
        fix: &str,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                self.add(id, "pass", good, "");
                Some(value)
            }
            Err(_) => {
                self.add(id, "fail", bad, fix);
                None
            }
        }
    }
    // Only validators with controlled, value-free errors use this path. Broker and
    // filesystem errors remain generic because they can contain deployment secrets.
    fn configuration<T>(
        &mut self,
        id: &str,
        result: Result<T, String>,
        good: &str,
        bad: &str,
        fix: &str,
    ) -> Option<T> {
        match result {
            Ok(value) => {
                self.add(id, "pass", good, "");
                Some(value)
            }
            Err(reason) => {
                self.add(id, "fail", format!("{bad} {reason}."), fix);
                None
            }
        }
    }
    fn failed(&self) -> bool {
        self.checks.iter().any(|c| c.status == "fail")
    }
}
struct Target {
    id: String,
    connection: connection::Config,
    domain: String,
    writes: bool,
}
fn file(path: &Path, limit: u64) -> Result<Vec<u8>, ()> {
    let metadata = std::fs::metadata(path).map_err(|_| ())?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(());
    }
    let mut bytes = vec![];
    std::fs::File::open(path)
        .map_err(|_| ())?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ())?;
    if bytes.is_empty() || bytes.len() as u64 > limit {
        return Err(());
    }
    Ok(bytes)
}
fn configured(name: &str) -> bool {
    std::env::var_os(name).is_some()
}

pub async fn run(args: &[String]) -> i32 {
    if args == ["--help"] {
        println!("{HELP}");
        return 0;
    }
    let Ok(options) = Options::parse(args) else {
        eprintln!("{HELP}");
        return 2;
    };
    let mut report = Report {
        format: "natsui-doctor-v1",
        version: env!("CARGO_PKG_VERSION"),
        mode: "invalid",
        checks: vec![],
    };
    let policy=report.configuration("login.policy",nats_login::Policy::from_env(),"Login policy is valid.","NATS login settings conflict or contain invalid flags.","Use 0 or 1 for NATSUI_NATS_LOGIN and its options. Session-only requires NATSUI_NATS_LOGIN=1 and both NATSUI_NATS_LOGIN_SHARED_HISTORY=0 and NATSUI_NATS_LOGIN_SHARED_MONITORING=0.");
    if let Some(policy) = policy {
        report.mode = if policy.session_only {
            "session_only"
        } else if policy.enabled {
            "nats_login_with_collector"
        } else {
            "dashboard_identity_or_local"
        };
        report.add("login.session_only",if policy.session_only {"pass"}else if options.require_session {"fail"}else{"warning"},
            if policy.session_only {"Session-only mode is configured. History is private to each login and transient."}else{"The recommended NATS session-only path is not configured."},
            if policy.session_only {""}else{"For session-only mode, set NATSUI_NATS_LOGIN=1 and NATSUI_NATS_LOGIN_SESSION_ONLY=1, disable both sharing flags, remove collector credentials from URLs and unset NATSUI_CREDS, NATSUI_TLS_CERT and NATSUI_TLS_KEY in every profile. Recreate the container after deployment edits."});
        report.add("login.certificates",if policy.certificates {"unknown"}else{"disabled"},if policy.certificates {"Certificate sign-in is enabled locally. NATS identity mapping is not established by local configuration."}else{"Certificate sign-in is disabled."},"To use certificate sign-in, set NATSUI_NATS_LOGIN_CERTIFICATES=1 and configure NATS TLS verify_and_map. Mount a trusted CA as NATSUI_TLS_CA when required. Login supplies the user certificate and private key, not collector mounts.");
    }
    let auth=report.validation("dashboard.authentication",auth::Auth::from_env(),"Dashboard authentication settings parsed.","Dashboard authentication configuration could not be loaded.","Correct the login policy or mount NATSUI_AUTH_TOKEN_FILE as a readable file containing the generated 64-character key. Session-only NATS sign-in needs no dashboard key.");
    if let Some(auth) = &auth {
        if let Ok(origin) = std::env::var("NATSUI_PUBLIC_URL") {
            report.validation("dashboard.https",auth.configure_origin(&origin),"HTTPS public-origin configuration is valid. Proxy routing and browser TLS remain untested.","Public-origin configuration is invalid.","Set NATSUI_PUBLIC_URL to an HTTPS origin without a path, query or credentials, and enable dashboard or NATS authentication.");
        } else {
            report.add("dashboard.https","warning","No public HTTPS origin is configured. This does not establish how container ports are exposed.","For shared access, terminate HTTPS at the trusted proxy and set NATSUI_PUBLIC_URL to its HTTPS origin. For local use, publish only to 127.0.0.1. Browser HTTPS and NATS TLS are separate.");
        }
        if let Some(path) = std::env::var_os("NATSUI_ACCESS_POLICY_FILE") {
            report.validation("dashboard.profile_policy",file(Path::new(&path),65536).and_then(|bytes|auth.configure_policy(&bytes).map_err(|_|())),"Dashboard profile policy parsed. It does not grant NATS permissions.","Dashboard profile policy is unreadable or invalid.","Mount valid NATSUI_ACCESS_POLICY_FILE JSON readably. Its allowlists apply to dashboard identities, not NATS accounts.");
        }
    }
    let writes = report
        .validation(
            "deployment.writes",
            deployment::flag("NATSUI_ALLOW_WRITES"),
            "Write flag parsed.",
            "Invalid write flag.",
            "NATSUI_ALLOW_WRITES must be 0 or 1.",
        )
        .unwrap_or(false);
    report.validation("deployment.bind",deployment::flag("NATSUI_CONTAINER"),"Bind-mode flag parsed.","Invalid bind flag.","NATSUI_CONTAINER must be 0 or 1. Container mode binds all container interfaces, so check host port publication separately.");
    report.validation("deployment.port",deployment::port(),"Listen port parsed. Port availability is not tested.","Invalid listen port.","NATSUI_PORT must be an integer between 0 and 65535. Use a fixed nonzero port for Compose port mappings.");
    report.validation(
        "storage.budget",
        deployment::history_budget(),
        "History budget parsed.",
        "Invalid history budget.",
        "NATSUI_HISTORY_MAX_MB must be an integer from 16 to 4096.",
    );
    let domain = report.validation(
        "deployment.domain",
        deployment::domain(),
        "JetStream domain syntax is valid.",
        "Invalid JetStream domain.",
        "NATSUI_DOMAIN must be one subject token without wildcards or whitespace.",
    );
    if !configured("NATSUI_URL") {
        report.add("profile.default.addresses", "warning", "NATSUI_URL is unset. The default is the dashboard process's loopback address on port 4222.", "For a separate NATS container, set NATSUI_URL to its service name and client port, for example nats://nats1:4222,nats://nats2:4222. Monitoring and route ports are different services.");
    }
    if policy.is_some_and(|p| !p.session_only) {
        report.validation("profile.default.http_monitoring", crate::monitoring::Monitor::from_env(),
            "HTTP monitoring configuration parsed. Endpoints were not contacted.",
            "HTTP monitoring configuration is invalid.",
            "Choose either NATSUI_MONITOR_URLS or NATSUI_MONITOR_CONFIG_FILE. Check endpoint origins, TLS and credential mounts. NATS client ports are not HTTP monitoring ports.");
    }
    let mut targets = vec![];
    if let Some(connection)=report.configuration("profile.default.connection",connection::Config::from_env(),"Default connection configuration parsed.","Default NATS addresses or mounted connection files are invalid.","Check NATSUI_URL client addresses and ports. Mount readable NATSUI_TLS_CA when private trust is required. In session-only mode, remove URL credentials and unset NATSUI_CREDS, NATSUI_TLS_CERT and NATSUI_TLS_KEY.")
        && let Some(domain)=domain {targets.push(Target{id:"default".into(),connection,domain,writes});}
    if let Some(profiles)=report.validation("profiles.file",crate::profiles::definitions(),"Additional profile definitions parsed.","Additional profile file is unreadable or invalid.","Mount NATSUI_PROFILES_FILE as valid JSON, at most 64 KiB, with at most seven unique profile IDs other than default. Each profile needs fixed NATS URLs.") {
        for p in profiles {
            if policy.is_some_and(|p| !p.session_only) {
                report.validation(&format!("profile.{}.http_monitoring", p.id), p.monitor(false),
                    "HTTP monitoring configuration parsed. Endpoints were not contacted.",
                    "Profile HTTP monitoring configuration is invalid.",
                    "Choose either monitor_urls or monitor_config_file, and check endpoint origins and credential mounts.");
            }
            targets.push(Target{id:p.id.clone(),connection:p.connection(),domain:p.domain,writes:p.allow_writes});}
    }
    for target in &targets {
        let id = format!("profile.{}", target.id);
        let valid = report.configuration(
            &format!("{id}.configuration"),
            target.connection.validate(),
            "Profile addresses and identity settings are valid.",
            "Profile connection configuration is invalid.",
            "Correct the profile URLs and credential fields. Client certificate and key fields must be paired, and URL credentials cannot be combined with a credentials file.",
        );
        if policy.is_some_and(|p| p.session_only) {
            report.validation(&format!("{id}.session_identity"),target.connection.validate_session_only(),"Session profile has no collector identity.","Session profile contains collector credentials or invalid connection settings.","Remove credentials from every profile URL and unset credentials, certificate and key fields. Keep server trust in ca or NATSUI_TLS_CA.");
        }
        if valid.is_some() {
            report.validation(&format!("{id}.tls_files"),target.connection.options().await,"Connection and TLS material parsed. Server trust and reachability still require a live probe.","Connection credentials or TLS material could not be parsed.","Check file mounts and process UID permissions. CA and certificate files accept PEM or single DER certificates. Keys must be matching unencrypted PEM. Do not print secret file contents.");
        } else {
            report.add(
                format!("{id}.tls_files"),
                "unknown",
                "TLS parsing was skipped because the profile configuration is invalid.",
                "Correct the configuration check before parsing mounted files.",
            );
        }
        if policy.is_some_and(|p| p.enabled) {
            report.add(format!("{id}.password_login"),if target.connection.for_user("validation","validation").is_ok(){"unknown"}else if policy.is_some_and(|p|p.certificates){"disabled"}else{"fail"},"Password login compatibility depends on the profile and NATS server. Authentication and API grants need an explicit probe.","Password login cannot inherit mounted collector client certificates. Use a credential-free session profile or the certificate login option.");
        }
        report.add(format!("{id}.operations"),if target.writes {"unknown"}else{"disabled"},if target.writes {"Reviewed writes are enabled locally. NATS authorization and server support are not verified."}else{"Reviewed writes are disabled in this profile."},"For reviewed operations, set NATSUI_ALLOW_WRITES=1 for default or allow_writes=true for an additional profile. Native operations need NATS 2.11+ and the specific operation API grants. No write is attempted by doctor.");
        if policy.is_some_and(|p| p.session_only) {
            report.add(format!("{id}.monitoring"),"unknown","Session node monitoring is available as an opt-in after login. HTTP monitoring settings are ignored in session-only mode.","Enable Session node monitoring on Nodes. The signed-in NATS identity needs system service access or configured imports plus reply-inbox subscriptions. NATS discovery does not discover HTTP monitoring endpoints.");
        }
    }
    if configured("NATSUI_MANAGED_CONFIG_FILE") || configured("NATSUI_OIDC_CONFIG_FILE") {
        report.add("deployment.other_modes","unknown","Optional controller or OIDC settings are present. This focused NATS-login check does not validate them.","Use the managed-deployment or OIDC guide. NATS sign-in does not grant dashboard administration or controller authority.");
    }
    let dir = std::env::var_os("NATSUI_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or("data".into());
    report.add("storage.persistence",if dir.exists() && !dir.is_dir() {"fail"}else{"unknown"},if dir.is_dir(){"Data directory exists. Writeability, SQLite integrity and durable volume backing are not tested."}else{"Data directory does not currently exist or is not a readable directory."},"Mount the complete /data directory persistently and grant the runtime UID write access. The image uses UID 10001. Even session-only deployments initialize dashboard-owned storage. Session observations remain in memory. Doctor does not create or open the database.");
    if options.probe {
        if let Some(target) = targets.iter().find(|t| t.id == options.profile) {
            probe(&mut report, target, &options, policy).await;
        } else {
            report.add("probe.profile","fail","Selected profile could not be loaded.","Choose default or a valid ID from NATSUI_PROFILES_FILE and fix its reported configuration errors.");
        }
    } else {
        report.add("probe.identity","unknown","No network connection was attempted. Broker authentication, API access and certificate mapping remain untested.","Run doctor --probe with --username and --password-file, or --certificate-file and --key-file. Credentials are read only from explicitly supplied files. Collector credentials are never used for this probe.");
    }
    let exit = if report.failed() { 1 } else { 0 };
    if options.json {
        println!("{}", serde_json::to_string_pretty(&report).unwrap());
    } else {
        println!(
            "natsui doctor {}\nMode: {}\nRecommended path: NATS sign-in with session-only history\n",
            report.version, report.mode
        );
        for check in &report.checks {
            println!(
                "[{}] {}: {}",
                check.status.to_uppercase(),
                check.id,
                check.detail
            );
            if !check.guidance.is_empty() {
                println!("  Guidance: {}", check.guidance);
            }
        }
        println!(
            "\nResult: {}. Unknown checks are not proof of readiness. No deployment settings were changed.",
            if exit == 0 {
                "no failed checks"
            } else {
                "failed checks require attention"
            }
        );
    }
    exit
}

async fn probe(
    report: &mut Report,
    target: &Target,
    options: &Options,
    policy: Option<nats_login::Policy>,
) {
    if target.connection.validate().is_err() {
        report.add(
            "probe.configuration",
            "fail",
            "The selected profile is invalid, so the probe was skipped.",
            "Correct the profile configuration before supplying an identity.",
        );
        return;
    }
    let connection = if let (Some(username), Some(path)) = (&options.username, &options.password) {
        let password = file(path, 4096)
            .and_then(|bytes| String::from_utf8(bytes).map_err(|_| ()))
            .map(|s| s.trim_end_matches(['\r', '\n']).to_owned());
        match password {
            Ok(password) if !password.is_empty() => target.connection.for_user(username, &password),
            _ => Err("Invalid probe password file".into()),
        }
    } else {
        if !policy.is_some_and(|p| p.certificates) {
            report.add("probe.certificate_mode","fail","Certificate login is not enabled for this deployment.","Set NATSUI_NATS_LOGIN=1 and NATSUI_NATS_LOGIN_CERTIFICATES=1 only for NATS servers that enforce certificate identity mapping.");
            return;
        }
        match (
            file(options.certificate.as_ref().unwrap(), 98304),
            file(options.key.as_ref().unwrap(), 32768),
        ) {
            (Ok(cert), Ok(key)) => target
                .connection
                .for_certificate(&cert, &key)
                .map(|(config, _)| config),
            _ => Err("Invalid probe certificate or key file".into()),
        }
    };
    let Some(connection)=report.validation("probe.credentials",connection,"Explicit probe identity parsed. No collector credentials are inherited.","Probe identity could not be loaded.","Supply a readable UTF-8 password file, or a PEM/DER certificate and matching unencrypted PEM key. Check mounted CA trust. Secret contents and paths are omitted from this report.")else{return;};
    let Some(client)=report.validation("probe.authentication",connection.connect_user().await,"NATS accepted the supplied identity on a server requiring authentication.","NATS authentication or connection failed.","Check client seed addresses, container DNS/network, credentials, CA chain and hostname, and whether the server requires authentication. Certificate sessions also require NATS verify_and_map. A failed connection does not isolate which of these is wrong.")else{return;};
    report.add("probe.server_operations",if crate::editing::supported(&client.server_info().version){"pass"}else{"warning"},"The connected server version was checked against the native-operation minimum.",if crate::editing::supported(&client.server_info().version){"Native-operation version requirement passed. Other cluster nodes and write permissions remain unverified."}else{"Native reviewed operations require NATS 2.11 or newer. Read availability is checked separately."});
    let prefix = if target.domain.is_empty() {
        "$JS.API".into()
    } else {
        format!("$JS.{}.API", target.domain)
    };
    request(
        report,
        &client,
        "probe.stream_list",
        &format!("{prefix}.STREAM.LIST"),
        json!({"offset":0}),
        "streams",
    )
    .await;
    if let Some(stream) = &options.stream {
        request(
            report,
            &client,
            "probe.stream_info",
            &format!("{prefix}.STREAM.INFO.{stream}"),
            json!({}),
            "config",
        )
        .await;
        request(
            report,
            &client,
            "probe.consumer_list",
            &format!("{prefix}.CONSUMER.LIST.{stream}"),
            json!({"offset":0}),
            "consumers",
        )
        .await;
        if let Some(consumer) = &options.consumer {
            request(
                report,
                &client,
                "probe.consumer_info",
                &format!("{prefix}.CONSUMER.INFO.{stream}.{consumer}"),
                json!({}),
                "config",
            )
            .await;
        }
    } else {
        report.add("probe.resource_reads","unknown","Resource-specific reads were not tested.","Add --stream NAME and optionally --consumer NAME for non-consuming info/list checks. Message bodies, application subscriptions and failure-watch subscriptions are not tested.");
    }
    request(
        report,
        &client,
        "probe.system_monitoring",
        "$SYS.REQ.SERVER.PING.VARZ",
        json!({}),
        "data",
    )
    .await;
    report.add("probe.scope","unknown","Only supplied-identity reads and the first reachable seed were exercised. One system reply does not prove full cluster coverage.","Successful list access does not prove every resource is visible. No publish, pause, deletion, consumer pull, acknowledgment or configuration change was attempted. Write grants and reverse-proxy behavior need separate verification.");
}

async fn request(
    report: &mut Report,
    client: &async_nats::Client,
    id: &str,
    subject: &str,
    body: Value,
    field: &str,
) {
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        client.request(subject.to_owned(), body.to_string().into()),
    )
    .await;
    let (status, detail) = match response {
        Ok(Ok(message)) if message.payload.len() <= 2_000_000 => classify(&message.payload, field),
        Ok(Ok(_)) => ("unknown", "Response exceeded the diagnostic size limit."),
        _ => (
            "unknown",
            "No usable response. Missing service, permissions, reply routing or connectivity may be responsible.",
        ),
    };
    let guidance = if status == "pass" {
        "This verifies this read request only. No response contents or resource names are included."
            .into()
    } else {
        format!(
            "If this capability is intended, allow publish to {subject} and subscribe to the reply inbox (_INBOX.>). Check the selected account and JetStream domain. System monitoring also needs system-account service access or configured exports/imports. A timeout alone does not prove a permission denial."
        )
    };
    report.add(id, status, detail, guidance);
}
fn classify(bytes: &[u8], field: &str) -> (&'static str, &'static str) {
    let Ok(value) = serde_json::from_slice::<Value>(bytes) else {
        return ("unknown", "Response was not valid JSON.");
    };
    if value["error"]["code"] == 401 || value["error"]["code"] == 403 {
        return (
            "fail",
            "The service explicitly rejected this request as unauthorized.",
        );
    }
    if !value["error"].is_null() {
        return (
            "unknown",
            "The service returned an API error. Resource existence and feature availability are not established.",
        );
    }
    let usable = match field {
        "streams" | "consumers" => value[field].is_array(),
        "config" => value[field].is_object(),
        "data" => {
            value[field].is_object()
                && value["server"]["id"]
                    .as_str()
                    .is_some_and(telemetry::valid_token)
                && value[field]["server_id"]
                    .as_str()
                    .is_some_and(telemetry::valid_token)
        }
        _ => false,
    };
    if usable {
        (
            "pass",
            "A usable response was received for this read request.",
        )
    } else if matches!(field, "streams" | "consumers")
        && value[field].is_null()
        && value["total"] == 0
    {
        ("pass", "An explicit empty inventory response was received.")
    } else {
        (
            "unknown",
            "Response did not contain the expected observation fields.",
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probe_arguments_require_explicit_exclusive_identity() {
        for args in [
            vec!["--probe"],
            vec!["--password-file", "secret"],
            vec![
                "--probe",
                "--username",
                "reader",
                "--password-file",
                "p",
                "--stream",
                ">",
            ],
            vec![
                "--probe",
                "--username",
                "r",
                "--password-file",
                "p",
                "--certificate-file",
                "c",
                "--key-file",
                "k",
            ],
        ] {
            assert!(
                Options::parse(&args.into_iter().map(str::to_owned).collect::<Vec<_>>()).is_err()
            );
        }
        assert!(Options::parse(&["--json".into(), "--require-session-only".into()]).is_ok());
    }
    #[test]
    fn missing_or_denied_data_is_not_empty_success() {
        assert_eq!(
            classify(
                br#"{"error":{"code":403,"description":"SECRET"}}"#,
                "streams"
            )
            .0,
            "fail"
        );
        assert_eq!(
            classify(br#"{"error":{"code":404}}"#, "streams").0,
            "unknown"
        );
        assert_eq!(classify(br#"{}"#, "streams").0, "unknown");
        for (payload, field) in [
            (br#"{"streams":{}}"#.as_slice(), "streams"),
            (br#"{"streams":{},"total":0}"#.as_slice(), "streams"),
            (br#"{"config":[]}"#.as_slice(), "config"),
            (br#"{"data":{}}"#.as_slice(), "data"),
        ] {
            assert_eq!(classify(payload, field).0, "unknown");
        }
        assert_eq!(
            classify(
                br#"{"server":{"id":"node"},"data":{"server_id":"node"}}"#,
                "data"
            )
            .0,
            "pass"
        );
        assert_eq!(classify(br#"{"total":0}"#, "streams").0, "pass");
        assert_eq!(classify(br#"{"streams":[]}"#, "streams").0, "pass");
    }
}
