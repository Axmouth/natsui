use crate::telemetry::Snapshot;
use serde_json::{Value, json};

// Identity and compact counters are sufficient for trends; payloads and full
// configuration are deliberately excluded from historical samples.
pub fn project(snapshot: &Snapshot, monitoring: &Value) -> Value {
    let streams:Vec<Value>=snapshot.streams.iter().map(|s|{
        let name=&s["config"]["name"];
        let largest=snapshot.consumers.iter().filter(|c|c["stream_name"]==*name).filter_map(|c|c["num_pending"].as_u64().map(|n|(n,c))).max_by_key(|(n,_)|*n);
        json!({"name":name,"created":s["created"],"messages":s["state"]["messages"],"bytes":s["state"]["bytes"],"last_seq":s["state"]["last_seq"],
            "pending":if snapshot.status=="complete"{largest.map(|(n,_)|n)}else{None},"consumer":largest.map(|(_,c)|&c["name"]),
            "rate_eligible":s["config"]["mirror"].is_null()&&s["config"]["sources"].is_null()&&!s["config"]["allow_rollup_hdrs"].as_bool().unwrap_or(false)})
    }).collect();
    let consumers: Vec<Value> = snapshot.consumers.iter().map(|c| {
        let stream = snapshot.streams.iter().find(|s| s["config"]["name"] == c["stream_name"]);
        json!({"name":c["name"],"stream":c["stream_name"],"created":c["created"],"stream_created":stream.map(|s| &s["created"]),
            "pending":c["num_pending"],"ack_pending":c["num_ack_pending"],"redelivered":c["num_redelivered"],"waiting":c["num_waiting"],
            "delivered":c["delivered"]["consumer_seq"],"ack_floor":c["ack_floor"]["consumer_seq"],"max_ack_pending":c["config"]["max_ack_pending"]})
    }).collect();
    json!({"streams":streams,"consumers":consumers,"monitoring":monitoring})
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn backlog_projection_is_maximum_and_partial_is_unknown() {
        let mut s = crate::telemetry::demo("test");
        let r = project(&s, &json!({}));
        assert_eq!(r["streams"][0]["pending"], 82431);
        s.status = "partial".into();
        assert!(project(&s, &json!({}))["streams"][0]["pending"].is_null());
    }
}
