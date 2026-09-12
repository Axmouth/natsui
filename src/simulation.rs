use crate::telemetry::{Snapshot, now};
use serde_json::json;

// The cycle models accumulated work rather than independently randomizing
// gauges. The same deterministic model is used by the standalone web demo.
pub fn snapshot(scope: &str, elapsed: u64) -> Snapshot {
    let tick = elapsed % 120;
    let (phase, explanation, extra) = match tick {
        0..=19 => (
            "Steady traffic",
            "Producers and workers are keeping pace. Small backlogs are normal.",
            tick * 20,
        ),
        20..=39 => (
            "Slow billing worker",
            "Billing processes less than arrives; fulfillment continues independently.",
            400 + (tick - 20) * 450,
        ),
        40..=59 => (
            "Order burst",
            "A short producer burst grows the billing and warehouse backlogs.",
            9400 + (tick - 40) * 650,
        ),
        _ => (
            "Recovery",
            "Production returns to baseline and workers drain accumulated work.",
            22400u64.saturating_sub((tick - 60) * 374),
        ),
    };
    let mut streams = Vec::new();
    let mut consumers = Vec::new();
    for (index, (name, subject, rate, factor)) in [
        ("ORDERS", "orders.created", 300, 1),
        ("PAYMENTS", "payments.captured", 120, 3),
        ("JOBS", "jobs.resize", 180, 2),
    ]
    .into_iter()
    .enumerate()
    {
        let last = 100_000 + elapsed * rate;
        let pending = 600 + extra / factor;
        let acks = if (20..60).contains(&tick) {
            800 + tick * 5
        } else {
            30 + tick % 11
        };
        let retained = if name == "JOBS" {
            pending + acks
        } else {
            last.min(200_000)
        };
        streams.push(json!({"cluster":{"name":"simulated-cluster","leader":format!("demo-{}",index+1),"replicas":(1..=3).filter(|n|*n!=index+1).map(|n|json!({"name":format!("demo-{n}"),"current":true,"lag":0,"offline":false})).collect::<Vec<_>>()},"created":"2026-09-11T00:00:00Z","config":{"name":name,"subjects":[subject],"retention":if name=="JOBS" {"workqueue"} else {"limits"},"storage":"file","num_replicas":3,"max_msgs":200000,"metadata":{"natsui":"simulation"}},"state":{"messages":retained,"bytes":retained*220,"first_seq":last-retained+1,"last_seq":last,"consumer_count":if name=="JOBS" {1} else {2}}}));
        for worker in 0..if name == "JOBS" { 1 } else { 2 } {
            let backlog = if worker == 0 {
                pending
            } else {
                20 + (tick * 13 + index as u64 * 5) % 190
            };
            let outstanding = if worker == 0 { acks } else { 8 + tick % 8 };
            let label = match (index, worker) {
                (0, 0) => "billing-worker",
                (0, _) => "fulfillment",
                (1, 0) => "settlement",
                (1, _) => "receipts",
                _ => "image-workers",
            };
            consumers.push(json!({"created":"2026-09-11T00:00:00Z","stream_name":name,"name":label,"num_pending":backlog,"num_ack_pending":outstanding,"num_redelivered":if (20..60).contains(&tick)&&worker==0 {tick-19} else {0},"num_waiting":if (20..60).contains(&tick)&&worker==0 {0}else{1},"config":{"ack_policy":"explicit","max_ack_pending":1200,"max_deliver":5,"filter_subject":subject},"delivered":{"consumer_seq":last-backlog,"stream_seq":last-backlog},"ack_floor":{"stream_seq":last-backlog-outstanding}}));
        }
    }
    Snapshot {
        observed_at: now(),
        scope: scope.into(),
        demo: true,
        status: "complete".into(),
        issues: vec![],
        streams,
        consumers,
        scenario: Some(
            json!({"phase":phase,"description":explanation,"second":tick,"duration":120,"source":"simulation"}),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn lifecycle_grows_and_recovers_without_impossible_counts() {
        let pending = |t| {
            snapshot("test", t).consumers[0]["num_pending"]
                .as_u64()
                .unwrap()
        };
        assert!(pending(55) > pending(25));
        assert!(pending(115) < pending(65));
        for t in 0..240 {
            let s = snapshot("test", t);
            for c in &s.consumers {
                assert!(
                    c["ack_floor"]["stream_seq"].as_u64().unwrap()
                        <= c["delivered"]["stream_seq"].as_u64().unwrap()
                );
            }
            for stream in &s.streams {
                assert!(stream["state"]["messages"].as_u64().unwrap() <= 200000);
            }
        }
    }
}
