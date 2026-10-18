use crate::{db::Db, grid, state::*, store};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{fs, path::Path, time::Instant};
pub fn benchmark_pack(cells: usize, leads: usize, repeats: usize) -> Result<Value> {
    anyhow::ensure!(
        (1..=8192).contains(&cells) && (1..=141).contains(&leads) && (1..=100).contains(&repeats),
        "Invalid packing benchmark size"
    );
    let (levels, fields) = (30, 6);
    let source: Vec<Vec<f32>> = (0..leads)
        .map(|time| {
            (0..cells * levels * fields)
                .map(|i| {
                    f32::from_bits(0x3f000000 | (((i * 31 + time * 8191) as u32) & 0x007fffff))
                })
                .collect()
        })
        .collect();
    let pointers: Vec<_> = source.iter().map(|v| v.as_ptr()).collect();
    let mut expected = vec![0f32; cells * levels * fields * leads];
    unsafe {
        crate::pack::pack_tiled::<{ usize::MAX }>(
            pointers.as_ptr(),
            expected.as_mut_ptr(),
            cells,
            levels,
            fields,
            leads,
        );
    }
    let mut actual = vec![0f32; expected.len()];
    type Kernel = unsafe fn(*const *const f32, *mut f32, usize, usize, usize, usize) -> i32;
    let kernels: [(usize, Kernel); 6] = [
        (cells, crate::pack::pack_tiled::<{ usize::MAX }>),
        (8, crate::pack::pack_tiled::<8>),
        (16, crate::pack::pack_tiled::<16>),
        (32, crate::pack::pack_tiled::<32>),
        (64, crate::pack::pack_tiled::<64>),
        (128, crate::pack::pack_tiled::<128>),
    ];
    let mut measurements: Vec<Vec<f64>> = vec![vec![]; kernels.len()];
    for round in 0..=repeats {
        // Rotate order to avoid giving one kernel every first or last turn.
        for k in 0..kernels.len() {
            let i = (k + round) % kernels.len();
            let t = Instant::now();
            let bad = unsafe {
                kernels[i].1(
                    pointers.as_ptr(),
                    actual.as_mut_ptr(),
                    cells,
                    levels,
                    fields,
                    leads,
                )
            };
            let seconds = t.elapsed().as_secs_f64();
            anyhow::ensure!(
                bad == 0
                    && actual
                        .iter()
                        .zip(&expected)
                        .all(|(a, b)| a.to_bits() == b.to_bits()),
                "Packing benchmark mismatch"
            );
            std::hint::black_box(&actual);
            if round > 0 {
                measurements[i].push(seconds);
            }
        }
    }
    let results: Vec<_> = kernels
        .iter()
        .zip(measurements)
        .map(|((tile, _), mut seconds)| {
            seconds.sort_by(f64::total_cmp);
            json!({"tile_cells":tile,"seconds":seconds,"median_seconds":seconds[seconds.len()/2]})
        })
        .collect();
    Ok(
        json!({"cells":cells,"leads":leads,"levels":levels,"fields":fields,"repeats":repeats,"value_bytes":expected.len()*4,"all_bits_equal":true,"results":results,"kind":"resident synthetic packing only; no source or database I/O"}),
    )
}
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
pub fn verify(root: &Path, base: &str, compare: Option<&str>, full: bool) -> Result<Value> {
    // Keep the default HTTP selection and direct database reader on the same
    // manifest while the live updater publishes additional forecast hours.
    let _catalog = Lock::acquire(root, "catalog", false, false)?;
    let http = reqwest::blocking::Client::builder()
        .no_proxy()
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
            if let Some(compare) = compare {
                let expected: Value = http
                    .get(format!("{compare}/forecast"))
                    .query(&params)
                    .send()?
                    .error_for_status()?
                    .json()?;
                anyhow::ensure!(
                    equivalent(&v, &expected, ""),
                    "HTTP mismatch {name} at {la},{lo}"
                );
            }
            checks.push(json!({"model":name,"latitude":la,"longitude":lo,"cell_id":cell,"init_time":v["init_time"],"snapshot_id":v["snapshot_id"],"revision":v["revision"],"rows_sha256":format!("{:x}",Sha256::digest(serde_json::to_vec(&v["rows"])?)),"compared":compare.is_some()}));
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
pub fn staging(path: &Path) -> Result<Value> {
    let mut original = 0;
    let mut compressed = 0;
    let started = Instant::now();
    for e in fs::read_dir(path)? {
        let p = e?.path();
        if p.extension().is_some_and(|e| e == "npy") {
            let bytes = fs::read(p)?;
            let encoded = zstd::encode_all(bytes.as_slice(), 1)?;
            let decoded = zstd::decode_all(encoded.as_slice())?;
            anyhow::ensure!(decoded == bytes, "Lossless staging compression failed");
            original += bytes.len();
            compressed += encoded.len();
        }
    }
    anyhow::ensure!(
        original > 0,
        "Legacy compression measurement requires NPY inputs; new staging is already compressed"
    );
    Ok(
        json!({"raw_bytes":original,"zstd_bytes":compressed,"seconds":started.elapsed().as_secs_f64(),"lossless":true}),
    )
}

pub fn compare_stores(root: &Path, reference: &Path, samples: usize) -> Result<Value> {
    anyhow::ensure!(samples > 0, "Need samples");
    let _a = Lock::acquire(root, "catalog", false, false)?;
    let _b = Lock::acquire(reference, "catalog", false, false)?;
    let c = catalog(root)?;
    let expected = catalog(reference)?;
    anyhow::ensure!(
        !c.current.is_empty() && c.current.keys().eq(expected.current.keys()),
        "Comparison catalogs must contain the same nonempty model set"
    );
    let db = Db::new()?;
    let mut models = serde_json::Map::new();
    for (name, e) in c.current {
        let old = expected
            .current
            .get(&name)
            .context("Reference model missing")?;
        let m = read_json(descriptor(root, &e)?.join("metadata.json"))?;
        let om = read_json(descriptor(reference, old)?.join("metadata.json"))?;
        let reader = crate::runs::Reader::load(root, &e, &m)?;
        let original_reader = crate::runs::Reader::load(reference, old, &om)?;
        for key in [
            "dataset",
            "init_time",
            "snapshot_id",
            "leads",
            "variables",
            "surface_variables",
            "units",
        ] {
            anyhow::ensure!(
                equivalent(&m[key], &om[key], key),
                "Source identity/schema mismatch {name} {key}"
            );
        }
        anyhow::ensure!(
            numeric_array_eq(&m["levels"], &om["levels"]),
            "Pressure levels differ"
        );
        let arrays = crate::stage::load_cells(&descriptor(root, &e)?)?;
        let oa = crate::stage::load_cells(&descriptor(reference, old)?)?;
        anyhow::ensure!(
            arrays["cell_id"].bytes == oa["cell_id"].bytes,
            "Cell selection differs {name}"
        );
        let mut max_error = 0f64;
        for field in ["latitude", "longitude"] {
            anyhow::ensure!(
                arrays[field].bytes.len() == oa[field].bytes.len(),
                "Coordinate count mismatch"
            );
            for (a, b) in arrays[field]
                .bytes
                .as_chunks::<8>()
                .0
                .iter()
                .zip(oa[field].bytes.as_chunks::<8>().0.iter())
            {
                let a = f64::from_le_bytes(*a);
                let b = f64::from_le_bytes(*b);
                max_error = max_error.max((a - b).abs());
            }
        }
        anyhow::ensure!(
            max_error < 1e-9,
            "Projection coordinate mismatch {name}: {max_error}"
        );
        let ids: Vec<_> = arrays["cell_id"]
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        let mut compared = 0;
        for i in 0..samples {
            let id = ids[(i * 104729) % ids.len()];
            let (a, _) = reader.profile(&db, &e, &m, id)?;
            let (b, _) = original_reader.profile(&db, old, &om, id)?;
            anyhow::ensure!(
                equivalent(&json!(a), &json!(b), ""),
                "Source values differ {name} cell {id}"
            );
            compared += 1;
        }
        models.insert(name,json!({"cells":ids.len(),"complete_profiles_exact":compared,"max_coordinate_error_degrees":max_error,"logical_rows":ids.len()*m["leads"].as_array().unwrap().len(),"storage_rows":db.scalar(&format!("SELECT count() FROM {}.{}",ident(&e.database)?,ident(&e.table)?))?}));
    }
    Ok(json!({"time":now(),"models":models,"success":true}))
}

pub fn check_api(root: &Path, port: u16, compare: Option<&str>, full: bool) -> Result<Value> {
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
    let report = verify(root, &base, compare, full)?;
    drop(child);
    Ok(report)
}
