use crate::{db::Db, pack, stage::Array, state::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Mutex,
    },
    time::Instant,
};
struct StagedSample {
    profiles: Vec<f32>,
    surfaces: Vec<f32>,
}
fn sample_indices(nc: usize) -> BTreeSet<usize> {
    std::iter::once(0)
        .chain(std::iter::once(nc - 1))
        .chain((1..9).map(|i| ((i as u64 * 104729) % nc as u64) as usize))
        .collect()
}
fn array_check(name: &str, width: usize, bit: bool) -> String {
    let invalid = if bit {
        "bitAnd(x,2139095040)=2139095040"
    } else {
        "NOT isFinite(x)"
    };
    format!("length({name})!={width} OR arrayExists(x -> {invalid},{name})")
}
pub(crate) fn validate_inserted_batch(
    db: &Db,
    table: &str,
    m: &Value,
    lo: u32,
    hi: u32,
    rows: usize,
) -> Result<()> {
    ident(table)?;
    anyhow::ensure!(
        m["storage"]["layout"] == "cell_profile_v1" && m["storage"]["encoding"] == "float32_bits",
        "Batch validation needs the native bit layout"
    );
    let nt = m["leads"].as_array().context("Leads")?.len();
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let mut checks = vec!["revision_id != 1".to_owned()];
    for (f, name) in fields(m).iter().enumerate() {
        ident(name)?;
        checks.push(array_check(name, if f < nf { nl * nt } else { nt }, true));
    }
    anyhow::ensure!(db.scalar(&format!("SELECT toUInt8(count()={rows} AND countIf({})=0) FROM {DATABASE}.{table} WHERE cell_id>={lo} AND cell_id<={hi} SETTINGS max_threads=2,max_block_size=1024,compile_expressions=1,min_count_to_compile_expression=0",checks.join(" OR ")))?==1,"Malformed/non-finite or incomplete inserted range {lo}..{hi}");
    Ok(())
}
struct CandidateMerges<'a> {
    db: &'a Db,
    table: &'a str,
    paused: bool,
}
impl<'a> CandidateMerges<'a> {
    fn pause(db: &'a Db, table: &'a str) -> Result<Self> {
        db.command(&format!("SYSTEM STOP MERGES {DATABASE}.{table}"))?;
        Ok(Self {
            db,
            table,
            paused: true,
        })
    }
    fn resume(&mut self) -> Result<()> {
        self.db
            .command(&format!("SYSTEM START MERGES {DATABASE}.{}", self.table))?;
        self.paused = false;
        Ok(())
    }
}
impl Drop for CandidateMerges<'_> {
    fn drop(&mut self) {
        if self.paused {
            let _ = self.resume();
        }
    }
}
fn ids(path: &Path) -> Result<Vec<u32>> {
    let cells = crate::stage::load_cells(path)?;
    let a = cells.get("cell_id").context("Missing ids")?;
    anyhow::ensure!(a.dtype == "<u4" && a.bytes.len() % 4 == 0, "Invalid ids");
    let v: Vec<u32> = a
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    anyhow::ensure!(!v.is_empty(), "Empty cell index");
    anyhow::ensure!(v.windows(2).all(|w| w[0] < w[1]), "Unsorted ids");
    Ok(v)
}
fn encode(names: &[String], columns: &[&[f32]], widths: &[usize], ids: &[u32]) -> Result<Vec<u8>> {
    let ns: Vec<std::ffi::CString> = names
        .iter()
        .map(|v| std::ffi::CString::new(v.as_str()))
        .collect::<std::result::Result<_, _>>()?;
    let np: Vec<_> = ns.iter().map(|v| v.as_ptr()).collect();
    let cp: Vec<_> = columns.iter().map(|v| v.as_ptr() as *const u32).collect();
    let mut len = 0;
    let p = unsafe {
        pack::encode_native(
            np.as_ptr(),
            cp.as_ptr(),
            widths.as_ptr(),
            names.len(),
            ids.as_ptr(),
            ids.len(),
            1,
            &mut len,
        )
    };
    anyhow::ensure!(!p.is_null(), "Native encoding failed");
    Ok(unsafe { Box::from_raw(std::ptr::slice_from_raw_parts_mut(p, len)).into_vec() })
}
pub fn load(root: &Path, path: &Path, table: &str) -> Result<Value> {
    load_with_workers(root, path, table, None)
}
pub fn load_with_workers(
    root: &Path,
    path: &Path,
    table: &str,
    workers: Option<usize>,
) -> Result<Value> {
    ident(table)?;
    let started = Instant::now();
    let mut m = read_json(path.join("metadata.json"))?;
    for name in ["validated.json", "load-timings.json"] {
        let previous = path.join(name);
        if previous.exists() {
            fs::remove_file(previous)?;
        }
    }
    let cell_ids = ids(path)?;
    let nc = cell_ids.len();
    anyhow::ensure!(
        nc > 0 && m["cells"].as_u64() == Some(nc as u64),
        "Incorrect cell count"
    );
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let leads = m["leads"].as_array().context("Leads")?;
    let nt = leads.len();
    anyhow::ensure!(nt > 0 && nl > 0 && nf > 0, "Empty forecast axes");
    let workers = workers.unwrap_or(if m["model"] == "hrrr" { 4 } else { 1 });
    anyhow::ensure!(
        (1..=6).contains(&workers),
        "Loader workers must be 1 through 6"
    );
    let mut profiles = vec![];
    let mut surfaces = vec![];
    for l in leads {
        let lead = l.as_u64().context("Invalid lead")?;
        profiles.push(Array::open(
            &path.join(format!("lead-{lead:03}.npy")),
            &[nc, nl, nf],
        )?);
        surfaces.push(Array::open(
            &path.join(format!("surface-{lead:03}.npy")),
            &[nc, 2],
        )?);
    }
    let reader = profiles[0].format_name();
    let names = fields(&m);
    for n in &names {
        ident(n)?;
    }
    let db = Db::new()?;
    db.command(&format!("CREATE DATABASE IF NOT EXISTS {DATABASE}"))?;
    let mut columns = vec!["revision_id UInt32".to_owned(), "cell_id UInt32".to_owned()];
    let compression = if m["model"] == "hrrr" {
        "ZSTD(3)"
    } else {
        "ZSTD(1)"
    };
    for f in &names {
        columns.push(format!("{f} Array(UInt32) CODEC(T64,{compression})"));
    }
    db.command(&format!("CREATE TABLE {DATABASE}.{table} ({}) ENGINE=MergeTree PARTITION BY (revision_id,intDiv(cell_id,32768)) ORDER BY cell_id SETTINGS index_granularity=4,index_granularity_bytes=0,old_parts_lifetime=60",columns.join(",")))?;
    let mut merges = CandidateMerges::pause(&db, table)?;
    m["storage"] = json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":"float32_bits","codec":"t64","compression":compression,"index_granularity":4,"writer":"rust_native"});
    atomic(path.join("metadata.json"), &m)?;
    let batch = match m["model"].as_str() {
        Some("gfs") => 2048,
        Some("hrrr") => 4096,
        _ => 4096,
    };
    #[derive(Default)]
    struct Times {
        read: f64,
        pack: f64,
        encode: f64,
        insert: f64,
        finite: f64,
        cache_hint_failures: u64,
    }
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let workers = workers.min(nc.div_ceil(batch));
    let readers = workers.min(3);
    let widths: Vec<_> = (0..names.len())
        .map(|f| if f < nf { nl * nt } else { nt })
        .collect();
    let insert_start = Instant::now();
    let sample_cells = sample_indices(nc);
    let stage_samples = Mutex::new(BTreeMap::new());
    let times = std::thread::scope(|scope| -> Result<Times> {
        let mut handles = vec![];
        let (tx, rx) = mpsc::sync_channel(readers);
        let rx = Arc::new(Mutex::new(rx));
        struct StopOnPanic<'a>(&'a AtomicBool);
        impl Drop for StopOnPanic<'_> {
            fn drop(&mut self) {
                if std::thread::panicking() {
                    self.0.store(true, Ordering::Relaxed);
                }
            }
        }
        for _ in 0..readers {
            let (
                profiles,
                surfaces,
                names,
                widths,
                cell_ids,
                next,
                cancelled,
                sample_cells,
                stage_samples,
            ) = (
                &profiles,
                &surfaces,
                &names,
                &widths,
                &cell_ids,
                &next,
                &cancelled,
                &sample_cells,
                &stage_samples,
            );
            let tx = tx.clone();
            handles.push(scope.spawn(move || -> Result<Times> {
                let _panic = StopOnPanic(cancelled);
                let result = (|| -> Result<Times> {
                    let mut times = Times::default();
                    let mut scratch = crate::stage::Scratch::new()?;
                    let mut input: Vec<_> = (0..nt).map(|_| vec![0f32; batch * nl * nf]).collect();
                    let mut ground: Vec<_> = (0..nt).map(|_| vec![0f32; batch * 2]).collect();
                    let mut p = vec![0f32; batch * nl * nf * nt];
                    let mut g = vec![0f32; batch * 2 * nt];
                    loop {
                        if cancelled.load(Ordering::Relaxed) {
                            break;
                        }
                        let start = next.fetch_add(batch, Ordering::Relaxed);
                        if start >= nc {
                            break;
                        }
                        let c = batch.min(nc - start);
                        disk_guard(root, 0)?;
                        let t = Instant::now();
                        for (a, buffer) in profiles.iter().zip(&mut input) {
                            times.cache_hint_failures += a.read_into_with(
                                start * nl * nf,
                                &mut buffer[..c * nl * nf],
                                &mut scratch,
                            )?;
                        }
                        for (a, buffer) in surfaces.iter().zip(&mut ground) {
                            times.cache_hint_failures +=
                                a.read_into_with(start * 2, &mut buffer[..c * 2], &mut scratch)?;
                        }
                        times.read += t.elapsed().as_secs_f64();
                        // Retain the deterministic verification cells from the
                        // immutable staging bytes actually read by this load.
                        // This avoids decoding a whole block again for each
                        // single-cell comparison after insertion.
                        for &i in sample_cells.range(start..start + c) {
                            let local = i - start;
                            let sample = StagedSample {
                                profiles: input
                                    .iter()
                                    .flat_map(|v| {
                                        v[local * nl * nf..(local + 1) * nl * nf].iter().copied()
                                    })
                                    .collect(),
                                surfaces: ground
                                    .iter()
                                    .flat_map(|v| v[local * 2..(local + 1) * 2].iter().copied())
                                    .collect(),
                            };
                            stage_samples
                                .lock()
                                .unwrap_or_else(std::sync::PoisonError::into_inner)
                                .insert(i, sample);
                        }
                        let t = Instant::now();
                        let pp: Vec<_> = input.iter().map(|a| a.as_ptr()).collect();
                        let gp: Vec<_> = ground.iter().map(|a| a.as_ptr()).collect();
                        let bad = unsafe {
                            pack::pack_neon(pp.as_ptr(), p.as_mut_ptr(), c, nl, nf, nt)
                                | pack::pack_neon(gp.as_ptr(), g.as_mut_ptr(), c, 1, 2, nt)
                        };
                        anyhow::ensure!(bad == 0, "Non-finite staging");
                        times.pack += t.elapsed().as_secs_f64();
                        let t = Instant::now();
                        let cols: Vec<&[f32]> = p[..c * nl * nf * nt]
                            .chunks_exact(c * nl * nt)
                            .chain(g[..c * 2 * nt].chunks_exact(c * nt))
                            .collect();
                        let body = encode(names, &cols, widths, &cell_ids[start..start + c])?;
                        times.encode += t.elapsed().as_secs_f64();
                        drop(cols);
                        let mut job = (start, c, body);
                        loop {
                            if cancelled.load(Ordering::Relaxed) {
                                break;
                            }
                            match tx.try_send(job) {
                                Ok(()) => break,
                                Err(mpsc::TrySendError::Full(returned)) => {
                                    job = returned;
                                    std::thread::sleep(std::time::Duration::from_millis(1));
                                }
                                Err(mpsc::TrySendError::Disconnected(_)) => {
                                    anyhow::bail!("Insert workers disconnected")
                                }
                            }
                        }
                    }
                    Ok(times)
                })();
                if result.is_err() {
                    cancelled.store(true, Ordering::Relaxed);
                }
                result
            }));
        }
        drop(tx);
        for _ in 0..workers {
            let (rx, db, cancelled, completed, model, m, cell_ids) = (
                rx.clone(),
                &db,
                &cancelled,
                &completed,
                &m["model"],
                &m,
                &cell_ids,
            );
            handles.push(scope.spawn(move || -> Result<Times> {
                let _panic = StopOnPanic(cancelled);
                let result = (|| -> Result<Times> {
                    let mut times = Times::default();
                    loop {
                        let job = rx.lock().unwrap_or_else(std::sync::PoisonError::into_inner).recv();
                        let (start,c,body) = match job {Ok(v)=>v,Err(_)=>break};
                        if cancelled.load(Ordering::Relaxed) {continue;}
                        let t = Instant::now();
                        db.insert(table,body)?;
                        times.insert += t.elapsed().as_secs_f64();
                        let t=Instant::now();
                        // Each private, disjoint range is checked from the
                        // database immediately, while its new parts are hot.
                        validate_inserted_batch(db,table,m,cell_ids[start],cell_ids[start+c-1],c)?;
                        times.finite+=t.elapsed().as_secs_f64();
                        let done = completed.fetch_add(c,Ordering::Relaxed)+c;
                        if start%(batch*10)==0 || done==nc {println!("{}",json!({"phase":"load","model":model,"cells":done,"total":nc,"workers":workers,"readers":readers}));}
                    }
                    Ok(times)
                })();
                if result.is_err(){cancelled.store(true,Ordering::Relaxed);}
                result
            }));
        }
        // Drain every worker before returning any error: failed candidates may
        // be dropped only after no thread can still insert into their table.
        let mut sum = Times::default();
        let mut error = None;
        for h in handles {
            match h
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Loader worker panicked")))
            {
                Ok(t) => {
                    sum.read += t.read;
                    sum.pack += t.pack;
                    sum.encode += t.encode;
                    sum.insert += t.insert;
                    sum.finite += t.finite;
                    sum.cache_hint_failures += t.cache_hint_failures;
                }
                Err(e) => {
                    cancelled.store(true, Ordering::Relaxed);
                    if error.is_none() {
                        error = Some(e);
                    }
                }
            }
        }
        if let Some(e) = error {
            Err(e)
        } else {
            Ok(sum)
        }
    })?;
    anyhow::ensure!(
        completed.load(Ordering::Relaxed) == nc,
        "Not every input cell was inserted"
    );
    let total_insert_seconds = insert_start.elapsed().as_secs_f64();
    let t = Instant::now();
    db.command("SYSTEM JEMALLOC PURGE")?;
    let stage_samples = stage_samples
        .into_inner()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let evidence = validate_impl(
        path,
        table,
        DATABASE,
        true,
        Some(&stage_samples),
        Some(completed.load(Ordering::Relaxed)),
    )?;
    merges.resume()?;
    let times = json!({"read_seconds":times.read,"pack_seconds":times.pack,"encode_seconds":times.encode,"insert_seconds":times.insert,"finite_validation_work_seconds":times.finite,"finite_validation_batches":nc.div_ceil(batch),"finite_validation_rows":nc,"total_insert_seconds":total_insert_seconds,"validation_seconds":t.elapsed().as_secs_f64(),"total_seconds":started.elapsed().as_secs_f64(),"storage_rows":nc,"logical_rows":nc*nt,"workers":workers,"readers":readers,"queued_batches":readers,"reader":reader,"pipeline":"bounded_readers_writers_validated_v4","candidate_merges_resumed":true,"input_cache_hint_failures":times.cache_hint_failures,"input_cache_policy":"discard_consumed_pages_linux_fadvise"});
    atomic(path.join("load-timings.json"), &times)?;
    Ok(evidence)
}
pub fn validate(path: &Path, table: &str, database: &str, staged: bool) -> Result<Value> {
    validate_impl(path, table, database, staged, None, None)
}
fn validate_impl(
    path: &Path,
    table: &str,
    database: &str,
    staged: bool,
    preloaded: Option<&BTreeMap<usize, StagedSample>>,
    checked_rows: Option<usize>,
) -> Result<Value> {
    ident(table)?;
    ident(database)?;
    let marker = path.join("validated.json");
    if marker.exists() {
        fs::remove_file(&marker)?;
    }
    let m = read_json(path.join("metadata.json"))?;
    let cell_ids = ids(path)?;
    let nc = cell_ids.len();
    anyhow::ensure!(
        m["cells"].as_u64() == Some(nc as u64),
        "Metadata cell count differs from index"
    );
    let nt = m["leads"].as_array().context("Leads")?.len();
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let profile = m["storage"]["layout"] == "cell_profile_v1";
    let db = Db::new()?;
    let storage_rows = db.scalar(&format!("SELECT count() FROM {database}.{table}"))?;
    anyhow::ensure!(
        storage_rows == if profile { nc } else { nc * nt } as u64,
        "Wrong physical row count"
    );
    let actual = db.bytes(&format!(
        "SELECT {}cell_id FROM {database}.{table} {}ORDER BY cell_id FORMAT RowBinary",
        if profile { "" } else { "DISTINCT " },
        ""
    ))?;
    let expected: Vec<u8> = cell_ids.iter().flat_map(|v| v.to_le_bytes()).collect();
    anyhow::ensure!(
        actual == expected,
        "Missing/duplicated/substituted cell ids"
    );
    let mut checks = vec!["revision_id != 1".to_owned()];
    let bit = m["storage"]["encoding"] == "float32_bits";
    let names = fields(&m);
    let mut types = HashTypes::new();
    for row in db.json(&format!(
        "SELECT name,type FROM system.columns WHERE database='{database}' AND table='{table}'"
    ))? {
        types.insert(
            row["name"].as_str().context("Name")?.to_owned(),
            row["type"].as_str().context("Type")?.to_owned(),
        );
    }
    for (f, name) in names.iter().enumerate() {
        ident(name)?;
        if profile || f < nf {
            let width = if profile {
                if f < nf {
                    nl * nt
                } else {
                    nt
                }
            } else {
                nl
            };
            let ty = if bit {
                "Array(UInt32)"
            } else {
                "Array(Float32)"
            };
            anyhow::ensure!(
                types.get(name).map(String::as_str) == Some(ty),
                "Wrong physical type {name}"
            );
            checks.push(array_check(name, width, bit));
        } else {
            checks.push(format!("NOT isFinite({name})"));
        }
    }
    if let Some(checked) = checked_rows {
        anyhow::ensure!(
            profile && bit && checked == nc && preloaded.is_some(),
            "Incomplete in-memory range validation proof"
        );
    } else {
        anyhow::ensure!(
        db.scalar(&format!(
            "SELECT count() FROM {database}.{table} WHERE {} SETTINGS max_threads=8,max_block_size=1024,compile_expressions=1,min_count_to_compile_expression=0",
            checks.join(" OR ")
        ))? == 0,
        "Malformed/non-finite database values"
    );
    }
    if !profile {
        // A global 80-million-key set exceeds the 2 GiB query budget.
        for range in cell_ids[0] / 32768..=cell_ids[nc - 1] / 32768 {
            let lo = range * 32768;
            let hi = lo + 32768;
            let cells =
                cell_ids.partition_point(|id| *id < hi) - cell_ids.partition_point(|id| *id < lo);
            anyhow::ensure!(db.scalar(&format!("SELECT uniqExact((cell_id,lead_hour)) FROM {database}.{table} WHERE cell_id>={lo} AND cell_id<{hi}"))? == (cells * nt) as u64, "Duplicate/missing legacy cell/lead keys in range {range}");
        }
        let counts=db.json(&format!("SELECT lead_hour,count() AS n FROM {database}.{table} GROUP BY lead_hour ORDER BY lead_hour"))?;
        anyhow::ensure!(
            counts.len() == nt
                && counts
                    .iter()
                    .zip(m["leads"].as_array().unwrap())
                    .all(|(r, l)| r["lead_hour"] == *l
                        && r["n"]
                            .as_str()
                            .and_then(|v| v.parse::<usize>().ok())
                            .or_else(|| r["n"].as_u64().map(|v| v as usize))
                            == Some(nc)),
            "Incorrect legacy lead coverage"
        );
    }
    let samples = sample_indices(nc);
    if let Some(cached) = preloaded {
        anyhow::ensure!(
            cached.keys().copied().collect::<BTreeSet<_>>() == samples
                && cached
                    .values()
                    .all(|v| v.profiles.len() == nt * nl * nf && v.surfaces.len() == nt * 2),
            "Incomplete staged verification samples"
        );
    }
    for &i in &samples {
        let (rows, _) = db.profile(database, table, &m, cell_ids[i])?;
        if staged {
            for (t, lead) in m["leads"].as_array().unwrap().iter().enumerate() {
                let lead = lead.as_u64().unwrap();
                let mut pv = vec![0f32; nl * nf];
                let mut gv = [0f32; 2];
                if let Some(cached) = preloaded {
                    pv.copy_from_slice(&cached[&i].profiles[t * nl * nf..(t + 1) * nl * nf]);
                    gv.copy_from_slice(&cached[&i].surfaces[t * 2..(t + 1) * 2]);
                } else {
                    let p = Array::open(&path.join(format!("lead-{lead:03}.npy")), &[nc, nl, nf])?;
                    let g = Array::open(&path.join(format!("surface-{lead:03}.npy")), &[nc, 2])?;
                    p.read_into(i * nl * nf, &mut pv)?;
                    g.read_into(i * 2, &mut gv)?;
                }
                for f in 0..nf {
                    let levels = rows[t][f + 1]
                        .as_array()
                        .context("Missing pressure values")?;
                    anyhow::ensure!(levels.len() == nl, "Incorrect pressure length");
                    for (l, value) in levels.iter().enumerate() {
                        let got = value.as_f64().context("Null profile")? as f32;
                        anyhow::ensure!(
                            got.to_bits() == pv[l * nf + f].to_bits(),
                            "Staged profile mismatch"
                        );
                    }
                }
                for f in 0..2 {
                    let got = rows[t][1 + nf + f].as_f64().context("Null surface")? as f32;
                    anyhow::ensure!(got.to_bits() == gv[f].to_bits(), "Staged surface mismatch");
                }
            }
        }
    }
    let evidence = json!({"rows":nc*nt,"storage_rows":storage_rows,"table":table,"database":database,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"samples":samples.len(),"staged_comparison":staged,"staged_sample_origin":if preloaded.is_some(){"immutable_ingestion_buffers"}else{"staging_files"},"full_values_checked":true,"finite_validation_mode":if checked_rows.is_some(){"each_inserted_cell_range"}else{"whole_table_scan"},"storage":m.get("storage"),"validated_at":now()});
    atomic(marker, &evidence)?;
    Ok(evidence)
}
type HashTypes = std::collections::HashMap<String, String>;
pub fn collect(root: &Path, grace: f64) -> Result<()> {
    let _l = Lock::acquire(root, "catalog", true, false)?;
    let mut c = catalog(root)?;
    let keep: std::collections::HashSet<_> = c
        .current
        .values()
        .chain(c.previous.values())
        .map(|e| e.revision.clone())
        .collect();
    let db = Db::new()?;
    let mut pending = vec![];
    for e in &c.retired {
        if keep.contains(&e.revision) || now() - e.retired_at.unwrap_or(now()) < grace {
            pending.push(e.clone());
            continue;
        }
        if e.owned {
            db.command(&format!(
                "DROP TABLE IF EXISTS {}.{} SYNC",
                ident(&e.database)?,
                ident(&e.table)?
            ))?;
        }
        let p = descriptor(root, e)?;
        if p.exists() {
            fs::remove_dir_all(p)?;
        }
    }
    c.retired = pending;
    atomic(root.join("catalog.json"), &c)?;
    Ok(())
}
pub fn discard(root: &Path, path: &Path) -> Result<()> {
    let c = catalog(root)?;
    let name = path
        .file_name()
        .context("No revision name")?
        .to_string_lossy();
    if c.current
        .values()
        .chain(c.previous.values())
        .chain(c.retired.iter())
        .any(|e| e.revision == name)
    {
        return Ok(());
    }
    if path.join("discarded.json").exists() {
        let discarded = read_json(path.join("discarded.json"))?;
        if now() - discarded["time"].as_f64().unwrap_or_else(now) > 7. * 86400. {
            fs::remove_dir_all(path)?;
        }
        return Ok(());
    }
    let e: Entry = serde_json::from_slice(&fs::read(path.join("entry.json"))?)?;
    if e.owned {
        Db::new()?.command(&format!(
            "DROP TABLE IF EXISTS {}.{} SYNC",
            ident(&e.database)?,
            ident(&e.table)?
        ))?;
    }
    clean_workspace(path)?;
    for name in ["cells.npz", "cells.bin.zst"] {
        let cells = path.join(name);
        if cells.exists() {
            fs::remove_file(cells)?;
        }
    }
    atomic(path.join("discarded.json"), &json!({"time":now()}))
}
