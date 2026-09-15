use serde_json::{Value, json};
use std::collections::BTreeSet;

fn pending(sample: &Value) -> Option<u64> {
    (sample["status"] == "complete")
        .then(|| sample["summary"]["largest"]["pending"].as_u64())
        .flatten()
}

// Extrema retain observed peaks without averaging overlapping consumer backlogs.
// Segment identities prevent reduction from joining across omitted failures.
pub fn aggregate(mut samples: Vec<Value>, from: u64, to: u64) -> Value {
    let total = samples.len();
    let mut segment = 0;
    let mut previous: Option<(u64, bool, Option<u64>)> = None;
    for sample in &mut samples {
        let at = sample["at"].as_u64().unwrap_or(from);
        let valid = pending(sample).is_some();
        let cadence = sample["interval_seconds"].as_u64().filter(|n| *n > 0);
        if previous.is_some_and(|(before, usable, previous_cadence)| {
            !usable
                || !valid
                || cadence.is_none()
                || previous_cadence.is_none()
                || at.saturating_sub(before) > previous_cadence.unwrap_or(0).saturating_mul(3)
        }) {
            segment += 1;
        }
        sample["segment"] = json!(segment);
        previous = Some((at, valid, cadence));
    }
    if total > 240 {
        let width = (to - from + 1).div_ceil(60);
        let mut bins: Vec<Vec<usize>> = vec![vec![]; 60];
        for (index, sample) in samples.iter().enumerate() {
            let bin = ((sample["at"].as_u64().unwrap_or(from) - from) / width).min(59) as usize;
            bins[bin].push(index);
        }
        let mut selected = BTreeSet::new();
        for bin in bins.iter().filter(|bin| !bin.is_empty()) {
            selected.insert(bin[0]);
            selected.insert(*bin.last().unwrap());
            let values: Vec<_> = bin
                .iter()
                .filter_map(|index| pending(&samples[*index]).map(|value| (*index, value)))
                .collect();
            if let Some((index, _)) = values.iter().min_by_key(|(_, value)| value) {
                selected.insert(*index);
            }
            if let Some((index, _)) = values.iter().max_by_key(|(_, value)| value) {
                selected.insert(*index);
            }
        }
        samples = samples
            .into_iter()
            .enumerate()
            .filter_map(|(index, sample)| selected.contains(&index).then_some(sample))
            .collect();
    }
    json!({"from":from,"to":to,"total":total,"aggregated":samples.len()<total,"truncated":false,"samples":samples})
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn full_range_preserves_extrema_and_breaks_across_omitted_failure() {
        let mut rows: Vec<_> = (0..600).map(|at| json!({"at":at,"status":"complete","interval_seconds":1,"summary":{"largest":{"pending":10,"name":"worker"}}})).collect();
        rows[123]["summary"]["largest"]["pending"] = json!(9999);
        rows[127]["summary"]["largest"]["pending"] = json!(0);
        rows[125]["status"] = json!("unavailable");
        let result = aggregate(rows, 0, 599);
        let points = result["samples"].as_array().unwrap();
        assert!(points.len() <= 240);
        assert_eq!(points.first().unwrap()["at"], 0);
        assert_eq!(points.last().unwrap()["at"], 599);
        assert_eq!(result["total"], 600);
        assert_eq!(result["aggregated"], true);
        let peak = points.iter().find(|p| p["at"] == 123).unwrap();
        let low = points.iter().find(|p| p["at"] == 127).unwrap();
        assert_eq!(peak["summary"]["largest"]["pending"], 9999);
        assert_eq!(low["summary"]["largest"]["pending"], 0);
        assert_ne!(peak["segment"], low["segment"]);
    }
    #[test]
    fn historical_cadence_and_legacy_rows_do_not_invent_continuity() {
        let sample = |at, cadence| json!({"at":at,"status":"complete","interval_seconds":cadence,"summary":{"largest":{"pending":1}}});
        let result = aggregate(
            vec![
                sample(0, Some(60)),
                sample(60, Some(1)),
                sample(90, Some(1)),
                sample(91, None),
                sample(92, Some(1)),
            ],
            0,
            100,
        );
        let rows = result["samples"].as_array().unwrap();
        assert_eq!(rows[0]["segment"], rows[1]["segment"]);
        for pair in rows[1..].windows(2) {
            assert_ne!(pair[0]["segment"], pair[1]["segment"]);
        }
    }
    #[test]
    fn sparse_samples_and_empty_windows_are_not_invented() {
        let rows = vec![
            json!({"at":5,"status":"complete","interval_seconds":1,"summary":{"largest":{"pending":2}}}),
            json!({"at":90,"status":"complete","interval_seconds":1,"summary":{"largest":{"pending":3}}}),
        ];
        let result = aggregate(rows, 0, 100);
        assert_eq!(result["samples"].as_array().unwrap().len(), 2);
        assert_ne!(
            result["samples"][0]["segment"],
            result["samples"][1]["segment"]
        );
        assert_eq!(result["aggregated"], false);
        assert_eq!(aggregate(vec![], 0, 100)["total"], 0);
    }
}
