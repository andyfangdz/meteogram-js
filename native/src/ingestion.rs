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
fn failure_kind(error: &str) -> &'static str {
    if error.contains("memory limit") {
        "Memory limit"
    } else if error.contains("cloud references") {
        "Cloud data not yet available"
    } else if error.contains("reference") {
        "Source data not yet available"
    } else {
        "Ingestion attempt failed"
    }
}
fn public_run(root: &Path, entry: &Entry, roles: &[&str], availability: &Value) -> Result<Value> {
    let mut summary = runs::summary(root, entry)?;
    summary.as_object_mut().unwrap().remove("revision");
    summary["roles"] = json!(roles);
    summary["source_ready"] = availability["sources"]
        .as_array()
        .and_then(|sources| {
            sources.iter().find(|s| {
                s["dataset"] == summary["dataset"] && s["init_time"] == summary["init_time"]
            })
        })
        .map(|s| s["ready_leads"].clone())
        .unwrap_or(Value::Null);
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
        let mut buckets = vec![0u64; 12];
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
            if event["phase"] == "lead_failed"
                && event["detail"]["model"] == name
                && t > time - 3600.
            {
                failed += 1;
                last_failure = json!({"time":t,"reason":failure_kind(event["detail"]["error"].as_str().unwrap_or(""))});
            }
        }
        models.push(json!({"model":name,"runs":summaries,"published_last_hour":buckets.iter().sum::<u64>(),"activity":buckets,"failed_last_hour":failed,"last_publication":last_publication,"last_failure":last_failure}));
    }
    let heartbeat = service["time"].as_f64();
    let running = heartbeat.is_some_and(|t| (0. ..30.).contains(&(time - t)))
        && service["children"]["update"]["running"] == true;
    Ok(
        json!({"generated_at":time,"worker_running":running,"heartbeat":heartbeat,"plan_at":plan.map(|p|p["time"].clone()),"last_cycle":{"time":update["time"],"published":update["completed_leads"],"failures":update["failures"].as_array().map(Vec::len),"maintenance_seconds":update["maintenance_seconds"],"discovery_seconds":update["discovery_seconds"],"ingestion_seconds":update["ingestion_seconds"]},"models":models}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
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
