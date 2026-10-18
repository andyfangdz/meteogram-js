use crate::state::*;
use anyhow::Result;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::PathBuf,
    time::{Duration, Instant},
};
#[derive(Default)]
struct Samples {
    times: Vec<f64>,
    errors: Vec<String>,
}
fn summary(
    data: &HashMap<String, Samples>,
    started: f64,
    free: u64,
    mem: u64,
    done: bool,
) -> Value {
    let models:serde_json::Map<String,Value>=data.iter().map(|(n,s)|{let mut ts=s.times.clone();ts.sort_by(f64::total_cmp);let p=|v:f64|ts.get(((ts.len().saturating_sub(1))as f64*v).ceil()as usize).copied();(n.clone(),json!({"successes":ts.len(),"errors":s.errors.len(),"error_examples":s.errors.iter().take(5).collect::<Vec<_>>(),"p50_ms":p(0.5),"p95_ms":p(0.95),"max_ms":ts.last()}))}).collect();
    json!({"started":started,"time":now(),"models":models,"minimum_free_bytes":free,"minimum_host_available_memory_bytes":mem,"complete":done,"scope":"One fixed NY point/model over loopback; does not establish publication or WAN latency"})
}
pub async fn observe(
    base: String,
    until: PathBuf,
    output: PathBuf,
    disk_root: PathBuf,
    max_seconds: u64,
) -> Result<()> {
    let started = now();
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(30))
        .build()?;
    let mut hashes = HashMap::new();
    let mut samples: HashMap<String, Samples> = MODELS
        .iter()
        .map(|n| ((*n).into(), Samples::default()))
        .collect();
    let (mut free, mut mem) = (u64::MAX, u64::MAX);
    loop {
        free = free.min(fs2::available_space(&disk_root)?);
        let text = fs::read_to_string("/proc/meminfo")?;
        if let Some(v) = text
            .lines()
            .find(|s| s.starts_with("MemAvailable:"))
            .and_then(|s| s.split_whitespace().nth(1))
            .and_then(|s| s.parse::<u64>().ok())
        {
            mem = mem.min(v * 1024);
        }
        for name in MODELS {
            let t = Instant::now();
            let result = async {
                let v: Value = http
                    .get(format!("{base}/forecast"))
                    .query(&[("model", name), ("lat", "41.013"), ("lon", "-74.027")])
                    .send()
                    .await?
                    .error_for_status()?
                    .json()
                    .await?;
                let key = format!(
                    "{name}:{}:{}:{}",
                    v["init_time"], v["snapshot_id"], v["revision"]
                );
                let hash = format!("{:x}", Sha256::digest(serde_json::to_vec(&v["rows"])?));
                if let Some(old) = hashes.insert(key, hash.clone()) {
                    anyhow::ensure!(old == hash, "Immutable revision returned different values");
                }
                Ok::<_, anyhow::Error>(())
            }
            .await;
            let entry = samples.get_mut(name).unwrap();
            match result {
                Ok(()) => entry.times.push(t.elapsed().as_secs_f64() * 1000.),
                Err(e) => entry.errors.push(e.to_string()),
            }
        }
        let complete = until.exists()
            && read_json(&until)
                .ok()
                .and_then(|v| v["time"].as_f64())
                .is_some_and(|v| v >= started);
        let expired = now() - started >= max_seconds as f64;
        atomic(&output, &summary(&samples, started, free, mem, complete))?;
        if complete || expired {
            break;
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
    Ok(())
}
