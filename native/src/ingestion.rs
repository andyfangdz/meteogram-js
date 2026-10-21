//! Public operational summary. Never return raw errors, paths, tokens, or process IDs.
use crate::{runs, state::*};
use anyhow::Result;
use serde_json::{json, Value};
use std::{
    fs::File,
    io::{Read, Seek, SeekFrom},
    path::Path,
};

fn optional(root: &Path, name: &str) -> Result<Value> {
    let path = root.join(name);
    if !path.exists() {
        return Ok(Value::Null);
    }
    read_json(path)
}
fn recent_events(root: &Path) -> Result<Vec<Value>> {
    let path = root.join("events.jsonl");
    if !path.exists() {
        return Ok(vec![]);
    }
    let mut file = File::open(path)?;
    let offset = file.metadata()?.len().saturating_sub(4 * 1024 * 1024);
    file.seek(SeekFrom::Start(offset))?;
    let mut bytes = Vec::new();
    file.take(4 * 1024 * 1024).read_to_end(&mut bytes)?;
    // The tail may begin/end inside a record while the updater appends.
    Ok(bytes
        .split(|b| *b == b'\n')
        .skip(usize::from(offset > 0))
        .filter_map(|line| serde_json::from_slice(line).ok())
        .collect())
}
fn source_wait(error: &str) -> bool {
    // Compatibility for historical events written before waits had their own phase.
    error.starts_with("Incomplete cloud references:")
        || error.starts_with("Incomplete source references:")
}
fn failure_kind(error: &str) -> &'static str {
    if error.contains("memory limit") {
        "Memory limit"
    } else {
        "Ingestion attempt failed"
    }
}
fn apply_source(summary: &mut Value, availability: &Value) {
    let source = availability["sources"].as_array().and_then(|sources| {
        sources
            .iter()
            .find(|s| s["dataset"] == summary["dataset"] && s["init_time"] == summary["init_time"])
    });
    summary["source_ready"] = source
        .map(|s| s["ready_leads"].clone())
        .unwrap_or(Value::Null);
    summary["cloud_ready"] = source
        .map(|s| s["cloud_ready"].clone())
        .unwrap_or(Value::Null);
}
fn public_run(root: &Path, entry: &Entry, roles: &[&str], availability: &Value) -> Result<Value> {
    let mut summary = runs::summary(root, entry)?;
    summary.as_object_mut().unwrap().remove("revision");
    summary["roles"] = json!(roles);
    let m = read_json(descriptor(root, entry)?.join("metadata.json"))?;
    let mut cloud_leads = Vec::new();
    if m["model"] == "hrrr" {
        for book in runs::books(&m)? {
            let (_, _, child) = runs::book_entry(root, &book)?;
            if child["native_clouds"]["version"] == 2 {
                cloud_leads.push(book.lead);
            }
        }
    }
    summary["cloud_leads"] = json!(cloud_leads);
    apply_source(&mut summary, availability);
    Ok(summary)
}
pub fn snapshot(root: &Path) -> Result<Value> {
    let time = now();
    let events = recent_events(root)?;
    let update = optional(root, "last-update.json")?;
    let service = optional(root, "service-status.json")?;
    let availability = optional(root, "availability.json")?;
    let plan = events
        .iter()
        .rev()
        .find(|e| e["phase"] == "ingestion_planned");
    let _lock = Lock::acquire(root, "catalog", false, false)?;
    let c = catalog(root)?;
    let mut models = Vec::new();
    for name in MODELS {
        let mut selected: Vec<(Entry, Vec<&str>)> = vec![];
        let mut add = |entry: &Entry, role| {
            if let Some((_, roles)) = selected
                .iter_mut()
                .find(|(e, _)| e.revision == entry.revision)
            {
                roles.push(role);
            } else {
                selected.push((entry.clone(), vec![role]));
            }
        };
        if let Some(e) = c.current.get(name) {
            add(e, "latest");
        }
        if let Some(e) = c.complete_runs.get(name).and_then(|v| v.first()) {
            add(e, "complete");
        }
        if name == "hrrr" {
            if let Some(e) = &c.hrrr_long {
                add(e, "long");
            }
        }
        for e in c.building.get(name).into_iter().flatten() {
            add(e, "building");
        }
        let mut summaries = Vec::new();
        for (entry, roles) in selected {
            let mut s = public_run(root, &entry, &roles, &availability)?;
            s["target"] = json!(plan.is_some_and(|p| p["completion_targets"]
                [s["dataset"].as_str().unwrap()][1]
                == s["init_time"]));
            summaries.push(s);
        }
        // Show source runs before their first hour is published as well.
        for source in availability["sources"]
            .as_array()
            .into_iter()
            .flatten()
            .filter(|s| s["model"] == name)
        {
            if !summaries
                .iter()
                .any(|s| s["dataset"] == source["dataset"] && s["init_time"] == source["init_time"])
            {
                let mut summary = json!({"dataset":source["dataset"],"init_time":source["init_time"],"available_leads":[],"expected_leads":source["expected_leads"],"missing_leads":source["expected_leads"],"complete":false,"target":false,"roles":["source"],"cloud_leads":[],"freshness":{"available":false}});
                apply_source(&mut summary, &availability);
                summary["target"] = json!(plan.is_some_and(|p| p["completion_targets"]
                    [source["dataset"].as_str().unwrap_or("")][1]
                    == source["init_time"]));
                summaries.push(summary);
            }
        }
        let mut buckets = vec![0u64; 12];
        let mut waiting = 0;
        let mut lags = Vec::new();
        let mut latest_lag = None;
        let mut failed = 0;
        let mut last_publication: Option<f64> = None;
        let mut last_failure = Value::Null;
        for event in &events {
            let Some(t) = event["time"].as_f64().filter(|t| *t <= time) else {
                continue;
            };
            if event["phase"] == "lead_published" && event["model"] == name {
                last_publication = Some(last_publication.map_or(t, |old| old.max(t)));
                if t > time - 3600. {
                    buckets[(((t - (time - 3600.)) / 300.).floor() as usize).min(11)] += 1;
                }
            }
            if event["phase"] == "lead_ingested" && event["model"] == name && t > time - 3600. {
                if let Some(lag) = event["lag_seconds"]
                    .as_f64()
                    .filter(|v| v.is_finite() && *v >= 0.)
                {
                    lags.push(lag);
                    latest_lag = Some(lag);
                }
            }
            if event["detail"]["model"] == name && t > time - 3600. {
                if event["phase"] == "lead_waiting"
                    || (event["phase"] == "lead_failed"
                        && source_wait(event["detail"]["error"].as_str().unwrap_or("")))
                {
                    waiting += 1;
                } else if event["phase"] == "lead_failed" {
                    failed += 1;
                    last_failure = json!({"time":t,"reason":failure_kind(event["detail"]["error"].as_str().unwrap_or(""))});
                }
            }
        }
        lags.sort_by(f64::total_cmp);
        let median = if lags.is_empty() {
            None
        } else {
            Some((lags[(lags.len() - 1) / 2] + lags[lags.len() / 2]) / 2.)
        };
        models.push(json!({"model":name,"runs":summaries,"published_last_hour":buckets.iter().sum::<u64>(),"activity":buckets,"failed_last_hour":failed,"waiting_last_hour":waiting,"lag_seconds":latest_lag,"median_lag_seconds":median,"lag_samples":lags.len(),"last_publication":last_publication,"last_failure":last_failure}));
    }
    let heartbeat = service["time"].as_f64();
    let running = heartbeat.is_some_and(|t| (0. ..30.).contains(&(time - t)))
        && service["children"]["update"]["running"] == true;
    Ok(
        json!({"generated_at":time,"worker_running":running,"heartbeat":heartbeat,"source_checked_at":availability["time"],"plan_at":plan.map(|p|p["time"].clone()),"last_cycle":{"time":update["time"],"published":update["completed_leads"],"waiting":update["waiting_leads"],"failures":update["failures"].as_array().map(|v| v.iter().filter(|e| !source_wait(e["error"].as_str().unwrap_or(""))).count()),"maintenance_seconds":update["maintenance_seconds"],"discovery_seconds":update["discovery_seconds"],"ingestion_seconds":update["ingestion_seconds"]},"models":models}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn source_waits_are_not_failures_and_lag_uses_measured_publications() {
        let root = tempfile::tempdir().unwrap();
        let t = now();
        let events = [
            json!({"time":t-10.,"phase":"lead_failed","detail":{"model":"hrrr","error":"Incomplete cloud references: private source key"}}),
            json!({"time":t-9.,"phase":"lead_waiting","detail":{"model":"hrrr","reason":"Cloud data not yet available"}}),
            json!({"time":t-8.,"phase":"lead_failed","detail":{"model":"hrrr","error":"Malformed cloud reference schema"}}),
            json!({"time":t-7.,"phase":"lead_ingested","model":"hrrr","lag_seconds":20.}),
            json!({"time":t-6.,"phase":"lead_ingested","model":"hrrr","lag_seconds":100.}),
            json!({"time":t-3601.,"phase":"lead_ingested","model":"hrrr","lag_seconds":900.}),
            json!({"time":t+100.,"phase":"lead_ingested","model":"hrrr","lag_seconds":900.}),
        ];
        std::fs::write(
            root.path().join("events.jsonl"),
            events
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        )
        .unwrap();
        atomic(root.path().join("last-update.json"), &json!({"failures":[{"error":"Incomplete source references: private"},{"error":"Invalid schema"}]})).unwrap();
        let result = snapshot(root.path()).unwrap();
        let model = &result["models"][0];
        assert_eq!(model["waiting_last_hour"], 2);
        assert_eq!(model["failed_last_hour"], 1);
        assert_eq!(model["lag_samples"], 2);
        assert_eq!(model["lag_seconds"], 100.);
        assert_eq!(model["median_lag_seconds"], 60.);
        assert_eq!(result["last_cycle"]["failures"], 1);
        assert!(!result.to_string().contains("private"));
    }
    #[test]
    fn unpublished_source_runs_expose_weather_and_cloud_readiness_separately() {
        let root = tempfile::tempdir().unwrap();
        atomic(root.path().join("availability.json"), &json!({"time":now(),"sources":[{"model":"hrrr","dataset":HRRR_SHORT,"init_time":"2026-10-21T11:00:00.000000000","expected_leads":[0,1,2],"ready_leads":[0,1],"cloud_ready":[0]}]})).unwrap();
        let result = snapshot(root.path()).unwrap();
        let run = &result["models"][0]["runs"][0];
        assert_eq!(run["roles"], json!(["source"]));
        assert_eq!(run["available_leads"], json!([]));
        assert_eq!(run["source_ready"], json!([0, 1]));
        assert_eq!(run["cloud_ready"], json!([0]));
        assert_eq!(run["complete"], false);
    }
    #[test]
    fn status_is_safe_without_history_and_errors_are_redacted() {
        let root = tempfile::tempdir().unwrap();
        let result = snapshot(root.path()).unwrap();
        assert_eq!(result["worker_running"], false);
        assert_eq!(result["models"].as_array().unwrap().len(), 4);
        std::fs::write(root.path().join("events.jsonl"), format!("{}\n{{unfinished", json!({"time":now()-1.,"phase":"lead_failed","detail":{"model":"gfs","error":"secret token in arbitrary upstream error"}}))).unwrap();
        let result = snapshot(root.path()).unwrap();
        assert_eq!(result["models"][1]["failed_last_hour"], 1);
        assert!(!result.to_string().contains("secret"));
        assert_eq!(
            result["models"][1]["last_failure"]["reason"],
            "Ingestion attempt failed"
        );
    }
    #[test]
    fn activity_ignores_future_events_and_counts_only_the_last_hour() {
        let root = tempfile::tempdir().unwrap();
        let t = now();
        let events = [-7200., -3590., -10., 600.]
            .map(|offset| {
                json!({"phase":"lead_published","model":"hrrr","time":t+offset}).to_string()
            })
            .join("\n");
        std::fs::write(root.path().join("events.jsonl"), events).unwrap();
        let result = snapshot(root.path()).unwrap();
        assert_eq!(result["models"][0]["published_last_hour"], 2);
        assert_eq!(result["models"][0]["activity"][0], 1);
        assert_eq!(result["models"][0]["activity"][11], 1);
    }
}
