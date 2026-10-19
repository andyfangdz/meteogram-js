use crate::{db::Db, grid, state::*, store};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Instant};
fn equivalent(a: &Value, b: &Value, key: &str) -> bool {
    if matches!(key, "query_ms" | "revision") {
        return true;
    }
    match (a, b) {
        (Value::Object(a), Value::Object(b)) => {
            a.len() == b.len()
                && a.iter()
                    .all(|(k, v)| b.get(k).is_some_and(|w| equivalent(v, w, k)))
        }
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b).all(|(v, w)| equivalent(v, w, key))
        }
        (Value::Number(a), Value::Number(b)) if matches!(key, "latitude" | "longitude") => {
            (a.as_f64().unwrap() - b.as_f64().unwrap()).abs() < 1e-9
        }
        (Value::Number(a), Value::Number(b)) => {
            a.as_f64().map(f64::to_bits) == b.as_f64().map(f64::to_bits)
        }
        _ => a == b,
    }
}
pub fn verify(root: &Path, base: &str, full: bool) -> Result<Value> {
    // Keep the default HTTP selection and direct database reader on the same
    // manifest while the live updater publishes additional forecast hours.
    let _catalog = Lock::acquire(root, "catalog", false, false)?;
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .default_headers(crate::api::client_headers(root)?)
        .timeout(std::time::Duration::from_secs(30))
        .build()?;
    let health = http
        .get(format!("{base}/health"))
        .send()?
        .error_for_status()?
        .json::<Value>()?;
    let c = catalog(root)?;
    let db = Db::new()?;
    let mut checks = vec![];
    for (name, e) in &c.current {
        let path = descriptor(root, e)?;
        let meta = read_json(path.join("metadata.json"))?;
        let reader = crate::runs::Reader::load(root, e, &meta)?;
        if full {
            store::validate(&path, &e.table, &e.database, false)?;
        }
        let cells = crate::stage::load_cells(&path)?;
        let ids: Vec<_> = cells["cell_id"]
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let coords = |name: &str| {
            cells[name]
                .bytes
                .as_chunks::<8>()
                .0
                .iter()
                .map(|b| f64::from_le_bytes(*b))
                .collect::<Vec<_>>()
        };
        let lat = coords("latitude");
        let lon = coords("longitude");
        let mut points = vec![(41.013, -74.027), (37., -120.), (40., -105.), (37., -97.)];
        for i in [
            0,
            ids.len() - 1,
            ids.len() / 2,
            ids.len() / 3,
            ids.len() / 7,
        ] {
            points.push((lat[i], lon[i]));
        }
        for (la, lo) in points {
            let (cell, _) = grid::resolve(&meta, &ids, la, lo)?;
            let params = [
                ("model", name.clone()),
                ("lat", la.to_string()),
                ("lon", lo.to_string()),
            ];
            let v: Value = http
                .get(format!("{base}/forecast"))
                .query(&params)
                .send()?
                .error_for_status()?
                .json()?;
            let (database_rows, _) = reader.profile(&db, e, &meta, cell)?;
            anyhow::ensure!(
                equivalent(&v["rows"], &json!(database_rows), ""),
                "HTTP/database values differ"
            );
            anyhow::ensure!(
                v["grid"]["cell_id"] == cell
                    && v["native_wind_frame"]
                        == if grid_relative(name) {
                            "grid-relative"
                        } else {
                            "earth-relative"
                        }
                    && v["init_time"] == meta["init_time"]
                    && v["snapshot_id"] == meta["snapshot_id"]
                    && v["rows"].as_array().context("No rows")?.len()
                        == meta["leads"].as_array().unwrap().len(),
                "HTTP identity/shape mismatch"
            );
            checks.push(json!({"model":name,"latitude":la,"longitude":lo,"cell_id":cell,"init_time":v["init_time"],"snapshot_id":v["snapshot_id"],"revision":v["revision"],"rows_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&v["rows"])?))}));
        }
    }
    for path in [
        "/forecast?model=hrrr&lat=NaN&lon=-74",
        "/forecast?model=bad&lat=41&lon=-74",
        "/forecast?model=hrrr&lat=91&lon=-74",
        "/forecast?model=hrrr&lon=-74",
    ] {
        anyhow::ensure!(
            http.get(format!("{base}{path}")).send()?.status().as_u16() == 400,
            "Invalid input accepted"
        );
    }
    for path in [
        "/forecast?model=hrrr&lat=23.999999&lon=-74",
        "/forecast?model=gfs&lat=41&lon=-125.000001",
        "/forecast?model=aifs&lat=50.000001&lon=-74",
        "/forecast?model=rrfs&lat=23.999999&lon=-74",
    ] {
        anyhow::ensure!(
            http.get(format!("{base}{path}")).send()?.status().as_u16() == 404,
            "Out-of-CONUS point accepted"
        );
    }
    let res = http
        .get(format!("{base}/forecast?model=aifs&lat=41&lon=-74"))
        .header("Accept-Encoding", "gzip")
        .send()?
        .error_for_status()?;
    anyhow::ensure!(
        res.headers()
            .get("content-encoding")
            .is_some_and(|v| v == "gzip"),
        "Gzip missing"
    );
    let compressed = res.bytes()?;
    let mut decoder = flate2::read::GzDecoder::new(compressed.as_ref());
    let value: Value = serde_json::from_reader(&mut decoder)?;
    anyhow::ensure!(value["model"] == "aifs", "Gzip mismatch");
    Ok(
        json!({"time":now(),"health":health,"profiles":checks,"invalid_inputs":4,"outside_region":4,"gzip_bytes":compressed.len(),"full_database_validation":full,"success":true}),
    )
}
pub fn benchmark(root: &Path, count: usize, clients: usize) -> Result<Value> {
    anyhow::ensure!(
        count > 0 && (1..=32).contains(&clients),
        "Invalid benchmark limits"
    );
    let c = catalog(root)?;
    let mut out = serde_json::Map::new();
    for (name, e) in c.current {
        let m = read_json(descriptor(root, &e)?.join("metadata.json"))?;
        let reader = std::sync::Arc::new(crate::runs::Reader::load(root, &e, &m)?);
        let cells = crate::stage::load_cells(&descriptor(root, &e)?)?;
        let ids: Vec<_> = cells["cell_id"]
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let db = Db::new()?;
        for i in 0..count {
            reader.profile(&db, &e, &m, ids[(i * 104729) % ids.len()])?;
        }
        let started = Instant::now();
        let mut handles = vec![];
        for worker in 0..clients {
            let db = db.clone();
            let e = e.clone();
            let m = m.clone();
            let ids = ids.clone();
            let reader = reader.clone();
            handles.push(std::thread::spawn(move || -> Result<Vec<f64>> {
                let mut times = vec![];
                for i in (worker..count).step_by(clients) {
                    let t = Instant::now();
                    reader.profile(&db, &e, &m, ids[(i * 104729) % ids.len()])?;
                    times.push(t.elapsed().as_secs_f64() * 1000.);
                }
                Ok(times)
            }));
        }
        let mut times = vec![];
        for h in handles {
            times.extend(
                h.join()
                    .map_err(|_| anyhow::anyhow!("Query thread panicked"))??,
            );
        }
        times.sort_by(f64::total_cmp);
        let p = |v: f64| times[((times.len() - 1) as f64 * v).ceil() as usize];
        out.insert(name,json!({"count":times.len(),"clients":clients,"p50_ms":p(0.5),"p95_ms":p(0.95),"p99_ms":p(0.99),"max_ms":times.last(),"queries_per_second":count as f64/started.elapsed().as_secs_f64()}));
    }
    Ok(json!({"time":now(),"warmup_queries_per_model":count,"models":out}))
}
pub fn check_api(root: &Path, port: u16, full: bool) -> Result<Value> {
    use std::{
        process::{Child, Command, Stdio},
        thread,
        time::Duration,
    };
    struct Server(Child);
    impl Drop for Server {
        fn drop(&mut self) {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(root.join("verification-api.log"))?;
    let mut child = Server(
        Command::new(std::env::current_exe()?)
            .arg("serve")
            .arg("--root")
            .arg(root)
            .arg("--port")
            .arg(port.to_string())
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .spawn()?,
    );
    let base = format!("http://127.0.0.1:{port}");
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
        .default_headers(crate::api::client_headers(root)?)
        .timeout(Duration::from_secs(1))
        .build()?;
    let mut ready = false;
    for _ in 0..100 {
        anyhow::ensure!(child.0.try_wait()?.is_none(), "Temporary API exited");
        if http
            .get(format!("{base}/health"))
            .send()
            .is_ok_and(|r| r.status().is_success())
        {
            ready = true;
            break;
        }
        thread::sleep(Duration::from_millis(100));
    }
    anyhow::ensure!(ready, "Temporary API did not become healthy");
    let report = verify(root, &base, full)?;
    drop(child);
    Ok(report)
}
