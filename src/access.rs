use crate::telemetry::valid_token;
use serde_json::{Value, json};
use std::time::Duration;

// NATS reports a connection's own permissions through $SYS.REQ.USER.INFO. The reply is the
// server's view of the identity, so matching it locally avoids probing denied subjects.
const USER_INFO: &str = "$SYS.REQ.USER.INFO";
const NODE_MONITORING: &str = "$SYS.REQ.SERVER.PING.VARZ";

#[derive(Clone, Debug, Default, PartialEq)]
struct Rules {
    allow: Vec<String>,
    deny: Vec<String>,
}
impl Rules {
    fn from(value: &Value) -> Self {
        let list = |key: &str| {
            value[key]
                .as_array()
                .map(|items| {
                    items
                        .iter()
                        .filter_map(Value::as_str)
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        Self {
            allow: list("allow"),
            deny: list("deny"),
        }
    }
    fn permits(&self, subject: &str) -> bool {
        (self.allow.is_empty() || self.allow.iter().any(|p| matches(p, subject)))
            && !self.deny.iter().any(|p| matches(p, subject))
    }
    // A wildcard request pattern is permitted only when one grant covers all of it and no
    // denial overlaps any part of it. Coverage spread across several grants is not combined.
    fn permits_all(&self, pattern: &str) -> bool {
        (self.allow.is_empty() || self.allow.iter().any(|p| covers(p, pattern)))
            && !self.deny.iter().any(|p| overlaps(p, pattern))
    }
}

fn covers(grant: &str, pattern: &str) -> bool {
    let mut grant = grant.split('.');
    let mut pattern = pattern.split('.');
    loop {
        match (grant.next(), pattern.next()) {
            (Some(">"), Some(_)) => return true,
            (_, Some(">")) => return false,
            (Some("*"), Some(_)) => {}
            (Some(g), Some(p)) if g == p && p != "*" => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

fn overlaps(a: &str, b: &str) -> bool {
    let mut a = a.split('.');
    let mut b = b.split('.');
    loop {
        match (a.next(), b.next()) {
            (Some(">"), Some(_)) | (Some(_), Some(">")) => return true,
            (Some(x), Some(y)) if x == "*" || y == "*" || x == y => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Access {
    pub user: String,
    pub account: String,
    restricted: bool,
    publish: Rules,
    nodes: bool,
    inventories: Vec<&'static str>,
}

pub fn matches(pattern: &str, subject: &str) -> bool {
    let mut pattern = pattern.split('.');
    let mut subject = subject.split('.');
    loop {
        match (pattern.next(), subject.next()) {
            (Some(">"), Some(_)) => return true,
            (Some("*"), Some(_)) => {}
            (Some(p), Some(s)) if p == s => {}
            (None, None) => return true,
            _ => return false,
        }
    }
}

impl Access {
    pub fn parse(reply: &Value) -> Option<Self> {
        let data = reply.get("data")?;
        if reply.get("error").is_some_and(|e| !e.is_null()) {
            return None;
        }
        let user = data["user"].as_str().filter(|u| !u.is_empty())?;
        let permissions = data.get("permissions").filter(|p| p.is_object());
        Some(Self {
            user: user.into(),
            account: data["account_name"]
                .as_str()
                .or(data["account"].as_str())
                .unwrap_or_default()
                .into(),
            restricted: permissions.is_some(),
            publish: permissions
                .map(|p| Rules::from(&p["publish"]))
                .unwrap_or_default(),
            nodes: false,
            inventories: vec![],
        })
    }
    pub fn restricted(&self) -> bool {
        self.restricted
    }
    pub fn can(&self, subject: &str) -> bool {
        self.publish.permits(subject)
    }
    pub fn lists_streams(&self, prefix: &str) -> bool {
        self.can(&format!("{prefix}.STREAM.LIST"))
    }
    pub fn sees_stream(&self, prefix: &str, stream: &str) -> bool {
        self.can(&format!("{prefix}.STREAM.INFO.{stream}"))
    }
    pub fn lists_consumers(&self, prefix: &str, stream: &str) -> bool {
        self.can(&format!("{prefix}.CONSUMER.LIST.{stream}"))
    }
    pub fn reads_messages(&self, prefix: &str, stream: &str) -> bool {
        self.can(&format!("{prefix}.STREAM.MSG.GET.{stream}"))
            || self.can(&format!("{prefix}.DIRECT.GET.{stream}"))
    }
    // Permission alone is not reachability: an application account also needs the system
    // service import, so discovery confirms that at least one server answers.
    pub fn monitors_nodes(&self) -> bool {
        self.nodes
    }
    // CONNZ, SUBSZ and ROUTEZ describe other identities' clients and interest, so each is
    // offered only with its own grant, even when node metrics are available.
    pub fn monitors(&self, subject: &str) -> bool {
        let endpoint = subject.rsplit('.').next().unwrap_or_default();
        self.nodes
            && self.can(subject)
            && (endpoint == "VARZ" || self.inventories.contains(&endpoint))
    }
    // Streams named literally in STREAM.INFO grants. Wildcard grants cannot be enumerated
    // without STREAM.LIST, so they are only honoured as a filter on a listed inventory.
    pub fn named_streams(&self, prefix: &str) -> Vec<String> {
        let head = format!("{prefix}.STREAM.INFO.");
        let mut names: Vec<String> = self
            .publish
            .allow
            .iter()
            .filter_map(|p| p.strip_prefix(&head))
            .filter(|name| valid_token(name))
            .filter(|name| self.sees_stream(prefix, name))
            .map(str::to_owned)
            .collect();
        names.sort();
        names.dedup();
        names
    }
    pub fn summary(&self, prefix: &str, streams: &[Value]) -> Value {
        let detail: Vec<Value> = streams
            .iter()
            .filter_map(|s| s["config"]["name"].as_str())
            .map(|name| {
                json!({"stream":name,"consumers":self.lists_consumers(prefix, name),"messages":self.reads_messages(prefix, name)})
            })
            .collect();
        json!({"source":"user_info","user":self.user,"account":self.account,"restricted":self.restricted,
            "lists_streams":self.lists_streams(prefix),"monitoring":self.nodes,
            "connections":self.monitors("$SYS.REQ.SERVER.PING.CONNZ"),"subscriptions":self.monitors("$SYS.REQ.SERVER.PING.SUBSZ"),
            "routes":self.monitors("$SYS.REQ.SERVER.PING.ROUTEZ"),"streams":detail})
    }
}

fn capability(id: &str, write: bool, needs: Vec<String>, access: Option<&Access>) -> Value {
    let granted = access.map(|a| needs.iter().all(|subject| a.publish.permits_all(subject)));
    json!({"id":id,"write":write,"needs":needs,"granted":granted})
}

// Every gated capability with the publish grants it needs, so a login can see what it has
// and what would enable the rest. Without known grants each capability is reported as
// unknown rather than assumed. NATS remains the authority for every request.
pub fn capabilities(
    access: Option<&Access>,
    prefix: &str,
    streams: &[Value],
    consumers: &[Value],
) -> Value {
    let one = |subject: &str| vec![subject.to_owned()];
    let mut nodes = capability("node_metrics", false, one(NODE_MONITORING), access);
    nodes["reachable"] = json!(access.map(|a| a.nodes));
    let cluster = vec![
        capability("own_permissions", false, one(USER_INFO), access),
        capability(
            "list_streams",
            false,
            one(&format!("{prefix}.STREAM.LIST")),
            access,
        ),
        nodes,
        capability(
            "connections",
            false,
            vec![
                "$SYS.REQ.SERVER.PING.CONNZ".into(),
                "$SYS.REQ.SERVER.*.CONNZ".into(),
            ],
            access,
        ),
        capability(
            "subscriptions",
            false,
            one("$SYS.REQ.SERVER.PING.SUBSZ"),
            access,
        ),
        capability("routes", false, one("$SYS.REQ.SERVER.PING.ROUTEZ"), access),
        capability(
            "create_stream",
            true,
            one(&format!("{prefix}.STREAM.CREATE.*")),
            access,
        ),
    ];
    let streams: Vec<Value> = streams
        .iter()
        .filter_map(|s| {
            let name = s["config"]["name"].as_str().filter(|n| valid_token(n))?;
            let subjects: Vec<String> = s["config"]["subjects"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect();
            let get = format!("{prefix}.STREAM.MSG.GET.{name}");
            let mut republish = subjects.clone();
            republish.push(get.clone());
            Some(json!({"stream":name,"capabilities":[
                capability("consumer_inventory", false, vec![format!("{prefix}.CONSUMER.LIST.{name}"), format!("{prefix}.CONSUMER.INFO.{name}.*")], access),
                capability("read_messages", false, vec![get], access),
                capability("publish", true, subjects, access),
                capability("republish", true, republish, access),
                capability("purge", true, one(&format!("{prefix}.STREAM.PURGE.{name}")), access),
                capability("delete_message", true, one(&format!("{prefix}.STREAM.MSG.DELETE.{name}")), access),
                capability("create_consumer", true, one(&format!("{prefix}.CONSUMER.CREATE.{name}.*")), access),
                capability("delete_stream", true, one(&format!("{prefix}.STREAM.DELETE.{name}")), access),
            ]}))
        })
        .take(300)
        .collect();
    let consumers: Vec<Value> = consumers
        .iter()
        .filter_map(|c| {
            let stream = c["stream_name"].as_str().filter(|n| valid_token(n))?;
            let name = c["name"].as_str().filter(|n| valid_token(n))?;
            Some(json!({"stream":stream,"consumer":name,"capabilities":[
                capability("pause_consumer", true, vec![format!("{prefix}.CONSUMER.INFO.{stream}.{name}"), format!("{prefix}.CONSUMER.PAUSE.{stream}.{name}")], access),
                capability("delete_consumer", true, vec![format!("{prefix}.CONSUMER.INFO.{stream}.{name}"), format!("{prefix}.CONSUMER.DELETE.{stream}.{name}")], access),
            ]}))
        })
        .take(500)
        .collect();
    json!({"known":access.is_some(),"restricted":access.map(|a| a.restricted),"cluster":cluster,"streams":streams,"consumers":consumers})
}

// Streams merged from other accounts carry natsui_account and are evaluated against that
// account's API prefix. Cluster capabilities describe the profile's own account.
pub fn capabilities_by_account(
    access: Option<&Access>,
    prefix: &str,
    streams: &[Value],
    consumers: &[Value],
) -> Value {
    let account = |v: &Value| v["natsui_account"].as_str().map(str::to_owned);
    let mut labels: Vec<Option<String>> = streams.iter().map(account).collect();
    labels.sort();
    labels.dedup();
    let mut result = capabilities(access, prefix, &[], &[]);
    for label in labels {
        let own: Vec<Value> = streams
            .iter()
            .filter(|s| account(s) == label)
            .cloned()
            .collect();
        let theirs: Vec<Value> = consumers
            .iter()
            .filter(|c| account(c) == label)
            .cloned()
            .collect();
        let prefix = label
            .as_deref()
            .map(crate::telemetry::account_prefix)
            .unwrap_or_else(|| prefix.to_owned());
        let part = capabilities(access, &prefix, &own, &theirs);
        for key in ["streams", "consumers"] {
            let items = part[key].as_array().cloned().unwrap_or_default();
            if let Some(target) = result[key].as_array_mut() {
                target.extend(items.into_iter().map(|mut item| {
                    item["account"] = json!(label);
                    item
                }));
            }
        }
    }
    result
}

pub async fn discover(client: &async_nats::Client) -> Option<Access> {
    let reply = tokio::time::timeout(Duration::from_secs(2), client.request(USER_INFO, "".into()))
        .await
        .ok()?
        .ok()?;
    let mut access = Access::parse(&serde_json::from_slice(&reply.payload).ok()?)?;
    if access.can(NODE_MONITORING) {
        access.nodes = reaches_nodes(client).await;
    }
    // Each inventory needs its own import as well as a grant, so a granted one is offered
    // only after a server answers it once.
    if access.nodes {
        for endpoint in ["CONNZ", "SUBSZ", "ROUTEZ"] {
            let subject = format!("$SYS.REQ.SERVER.PING.{endpoint}");
            if access.can(&subject) && answers(client, subject, "{\"limit\":1}").await {
                access.inventories.push(endpoint);
            }
        }
    }
    Some(access)
}

async fn answers(client: &async_nats::Client, subject: String, body: &'static str) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(1), client.request(subject, body.into())).await,
        Ok(Ok(_))
    )
}

pub async fn reaches_nodes(client: &async_nats::Client) -> bool {
    answers(client, NODE_MONITORING.to_owned(), "").await
}

#[cfg(test)]
mod tests {
    use super::*;

    fn system1() -> Access {
        Access::parse(&json!({"data":{"user":"system1","account":"SOC","permissions":{"publish":{
            "allow":["socstreams.case.>","$JS.API.STREAM.INFO.socstreams_cases","$JS.API.CONSUMER.LIST.socstreams_cases",
                "$JS.API.STREAM.MSG.GET.socstreams_cases","$JS.API.STREAM.INFO.retired","$SYS.REQ.USER.INFO"],
            "deny":["$JS.API.STREAM.INFO.retired","$JS.API.STREAM.DELETE.>"]},
            "subscribe":{"allow":["_INBOX.>"]}}}}))
        .unwrap()
    }

    #[test]
    fn subject_matching_follows_nats_wildcards() {
        assert!(matches("a.*.c", "a.b.c"));
        assert!(matches("a.>", "a.b.c"));
        assert!(!matches("a.>", "a"));
        assert!(!matches("a.*", "a.b.c"));
        assert!(!matches(
            "$JS.API.CONSUMER.LIST.s.>",
            "$JS.API.CONSUMER.LIST.s"
        ));
        assert!(matches(
            "$JS.API.CONSUMER.LIST.s",
            "$JS.API.CONSUMER.LIST.s"
        ));
    }

    #[test]
    fn restricted_identity_names_only_permitted_streams() {
        let access = system1();
        assert!(access.restricted());
        assert_eq!(access.named_streams("$JS.API"), vec!["socstreams_cases"]);
        assert!(!access.lists_streams("$JS.API"));
        assert!(access.lists_consumers("$JS.API", "socstreams_cases"));
        assert!(!access.lists_consumers("$JS.API", "other"));
        assert!(access.reads_messages("$JS.API", "socstreams_cases"));
        assert!(!access.can(NODE_MONITORING));
        assert!(!access.monitors_nodes());
    }

    #[test]
    fn identity_without_permissions_is_unrestricted() {
        let access = Access::parse(&json!({"data":{"user":"admin","account":"SOC"}})).unwrap();
        assert!(!access.restricted());
        assert!(access.lists_streams("$JS.API"));
        assert!(access.can(NODE_MONITORING));
        assert!(!access.monitors_nodes());
        assert!(access.named_streams("$JS.API").is_empty());
    }

    #[test]
    fn node_inventories_need_their_own_grants() {
        let mut access = Access::parse(
            &json!({"data":{"user":"system1","account":"SOC","permissions":{"publish":{
            "allow":["$SYS.REQ.USER.INFO","$SYS.REQ.SERVER.PING.VARZ"]}}}}),
        )
        .unwrap();
        assert!(!access.monitors(NODE_MONITORING));
        access.nodes = true;
        assert!(access.monitors(NODE_MONITORING));
        assert!(!access.monitors("$SYS.REQ.SERVER.PING.CONNZ"));
        assert!(!access.monitors("$SYS.REQ.SERVER.NABC.CONNZ"));
        let mut open = Access::parse(&json!({"data":{"user":"admin","account":"ADMIN"}})).unwrap();
        open.nodes = true;
        assert!(!open.monitors("$SYS.REQ.SERVER.PING.CONNZ"));
        open.inventories.push("CONNZ");
        assert!(open.monitors("$SYS.REQ.SERVER.PING.CONNZ"));
        assert!(open.monitors("$SYS.REQ.SERVER.NABC.CONNZ"));
        let summary = access.summary("$JS.API", &[]);
        assert_eq!(summary["monitoring"], true);
        assert_eq!(summary["connections"], false);
        assert_eq!(summary["routes"], false);
    }

    #[test]
    fn wildcard_requests_need_one_covering_grant_and_no_overlapping_denial() {
        assert!(covers("orders.>", "orders.*"));
        assert!(covers("orders.*", "orders.*"));
        assert!(!covers("orders.*", "orders.>"));
        assert!(!covers("orders.created", "orders.*"));
        assert!(covers("a.*.c", "a.b.c"));
        assert!(overlaps("orders.created", "orders.*"));
        assert!(overlaps("orders.>", "*.created"));
        assert!(!overlaps("orders.created", "payments.*"));
        assert!(!overlaps("a.b", "a.b.c"));
    }

    #[test]
    fn capabilities_report_grants_and_their_requirements() {
        let access = system1();
        let streams =
            [json!({"config":{"name":"socstreams_cases","subjects":["socstreams.case.>"]}})];
        let consumers = [json!({"stream_name":"socstreams_cases","name":"worker"})];
        let caps = capabilities(Some(&access), "$JS.API", &streams, &consumers);
        let stream = |id: &str| {
            caps["streams"][0]["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["id"] == id)
                .unwrap()
                .clone()
        };
        assert_eq!(stream("read_messages")["granted"], true);
        assert_eq!(stream("publish")["granted"], true);
        assert_eq!(stream("republish")["granted"], true);
        assert_eq!(stream("purge")["granted"], false);
        assert_eq!(
            stream("purge")["needs"],
            json!(["$JS.API.STREAM.PURGE.socstreams_cases"])
        );
        assert_eq!(stream("purge")["write"], true);
        assert_eq!(caps["consumers"][0]["capabilities"][0]["granted"], false);
        let unknown = capabilities(None, "$JS.API", &streams, &[]);
        assert_eq!(unknown["known"], false);
        assert!(unknown["cluster"][0]["granted"].is_null());
    }

    #[test]
    fn merged_streams_are_evaluated_against_their_account_prefix() {
        let access = Access::parse(&json!({"data":{"user":"system2","account":"SYSTEM2","permissions":{"publish":{
            "allow":["$JS.API.STREAM.INFO.alerts","$JS.API.STREAM.PURGE.alerts","$JS.system3.API.STREAM.INFO.events","$JS.system3.API.STREAM.MSG.GET.events"]}}}}))
        .unwrap();
        let streams = [
            json!({"config":{"name":"alerts","subjects":["alerts.>"]}}),
            json!({"config":{"name":"events","subjects":["events.>"]},"natsui_account":"system3"}),
        ];
        let caps = capabilities_by_account(Some(&access), "$JS.API", &streams, &[]);
        let find = |stream: &str, id: &str| {
            let entry = caps["streams"]
                .as_array()
                .unwrap()
                .iter()
                .find(|s| s["stream"] == stream)
                .unwrap();
            (
                entry["account"].clone(),
                entry["capabilities"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|c| c["id"] == id)
                    .unwrap()["granted"]
                    .clone(),
            )
        };
        assert_eq!(find("alerts", "purge"), (Value::Null, json!(true)));
        assert_eq!(
            find("events", "read_messages"),
            (json!("system3"), json!(true))
        );
        assert_eq!(find("events", "purge"), (json!("system3"), json!(false)));
        assert_eq!(access.named_streams("$JS.system3.API"), vec!["events"]);
    }

    #[test]
    fn error_replies_establish_nothing() {
        assert!(Access::parse(&json!({"error":{"code":500}})).is_none());
        assert!(Access::parse(&json!({"data":{},"error":{"code":403}})).is_none());
    }
}
