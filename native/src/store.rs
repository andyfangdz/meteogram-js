use crate::{db::Db, stage::Array, state::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::Path,
    sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc, Arc, Condvar, Mutex,
    },
    time::Instant,
};
struct StagedSample {
    profiles: Vec<f32>,
    surfaces: Vec<f32>,
}
struct InsertGate {
    available: Mutex<usize>,
    ready: Condvar,
}
impl InsertGate {
    fn enter(&self) -> InsertPermit<'_> {
        let mut slots = self
            .available
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *slots == 0 {
            slots = self
                .ready
                .wait(slots)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *slots -= 1;
        InsertPermit(self)
    }
}
struct InsertPermit<'a>(&'a InsertGate);
impl Drop for InsertPermit<'_> {
    fn drop(&mut self) {
        *self
            .0
            .available
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        self.0.ready.notify_one();
    }
}
fn sample_indices(nc: usize) -> BTreeSet<usize> {
    std::iter::once(0)
        .chain(std::iter::once(nc - 1))
        .chain((1..9).map(|i| ((i as u64 * 104729) % nc as u64) as usize))
        .collect()
}
pub(crate) fn validate_inserted_batch(
    db: &Db,
    table: &str,
    m: &Value,
    lo: u32,
    hi: u32,
    cells: usize,
    limits: &[Vec<u32>],
) -> Result<()> {
    ident(table)?;
    anyhow::ensure!(
        crate::packed::valid_layout(m),
        "Unsupported hourly tile layout"
    );
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let mut checks = vec!["revision_id != 1".to_owned(), tile_cell_count_check(m)?];
    let lead = m["storage"]["lead_hour"]
        .as_u64()
        .context("Missing lead partition")?;
    anyhow::ensure!(m["leads"] == json!([lead]), "Partition lead differs");
    checks.push(format!("lead_hour!={lead}"));
    for (f, name) in fields(m).iter().enumerate() {
        ident(name)?;
        checks.push(crate::packed::array_check(
            name,
            if f < nf { nl } else { 1 },
            &limits[f],
            crate::packed::GROUPS,
        ));
    }
    let rows = cells.div_ceil(crate::packed::CELLS);
    anyhow::ensure!(db.scalar(&format!("SELECT toUInt8(count()={rows} AND min(tile_id)={lo} AND max(tile_id)={hi} AND countIf({})=0) FROM {DATABASE}.{table} WHERE tile_id>={lo} AND tile_id<={hi} SETTINGS max_threads=2,max_block_size=4,compile_expressions=1,min_count_to_compile_expression=0",checks.join(" OR ")))?==1,"Malformed/non-finite or incomplete inserted range {lo}..{hi}");
    Ok(())
}
fn tile_cell_count_check(m: &Value) -> Result<String> {
    let cells = usize::try_from(m["cells"].as_u64().context("Cells")?)?;
    anyhow::ensure!(cells > 0, "Empty packed dataset");
    let tile_cells = crate::packed::tile_cells(m)?;
    let last = (cells - 1) / tile_cells;
    let final_cells = cells - last * tile_cells;
    Ok(format!(
        "length(cell_ids)!=if(tile_id={last},{final_cells},{})",
        tile_cells
    ))
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
pub fn load_lead(root: &Path, path: &Path, table: &str, lead: u16) -> Result<Value> {
    load_lead_with_workers(root, path, table, lead, None)
}
pub(crate) fn load_lead_with_workers(
    root: &Path,
    path: &Path,
    table: &str,
    partition_lead: u16,
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
    anyhow::ensure!(
        m["leads"] == json!([partition_lead]) && nl > 0 && nf > 0,
        "Expected exactly one forecast hour"
    );
    anyhow::ensure!(
        m["surface_variables"].as_array().map(Vec::len) == Some(2),
        "Expected two surface fields"
    );
    let nt = 1;
    let dictionary = crate::dictionary::Dictionary::load(path, &m)?;
    let dictionary_info = if crate::dictionary::staged(&m) {
        m["staging"]["dictionary"].clone()
    } else {
        m["storage"]["dictionary"].clone()
    };
    let dictionary_limits = (0..fields(&m).len())
        .map(|f| dictionary.limits(f))
        .collect::<Vec<_>>();
    anyhow::ensure!(
        dictionary_limits.iter().flatten().all(|n| *n <= 65536),
        "Hourly palettes must fit UInt16"
    );
    let profiles = vec![Array::open_as(
        &path.join(format!("lead-{partition_lead:03}")),
        &[nc, nl, nf],
        "uint16_le",
    )?];
    let surfaces = vec![Array::open_as(
        &path.join(format!("surface-{partition_lead:03}")),
        &[nc, 2],
        "uint16_le",
    )?];
    anyhow::ensure!(
        profiles.iter().chain(&surfaces).all(Array::short_codes),
        "Weather input requires UInt16 spatial-delta staging"
    );
    let reader = profiles[0].format_name();
    let input_bytes = profiles
        .iter()
        .chain(&surfaces)
        .map(Array::byte_len)
        .sum::<Result<u64>>()?;
    let memory_available = fs::read_to_string("/proc/meminfo").ok().and_then(|text| {
        text.lines().find_map(|line| {
            line.strip_prefix("MemAvailable:")
                .and_then(|v| v.split_whitespace().next()?.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        })
    });
    let prefetch_start = Instant::now();
    let prefetch = input_bytes <= 8 * 1024u64.pow(3)
        && memory_available.is_some_and(|available| available >= input_bytes + 18 * 1024u64.pow(3));
    if prefetch {
        let mut buffer = vec![0u8; 1024 * 1024];
        for array in profiles.iter().chain(&surfaces) {
            array.prefetch(&mut buffer)?;
        }
    }
    let prefetch_seconds = prefetch_start.elapsed().as_secs_f64();
    let names = fields(&m);
    for name in &names {
        ident(name)?;
    }
    let regional = m["model"].as_str().is_some_and(grid_relative);
    let workers = workers.unwrap_or(if regional { 6 } else { 1 });
    anyhow::ensure!(
        (1..=6).contains(&workers),
        "Loader workers must be 1 through 6"
    );
    let db = Db::new()?;
    db.command(&format!("CREATE DATABASE IF NOT EXISTS {DATABASE}"))?;
    db.command("SYSTEM JEMALLOC PURGE")?;
    let mut columns = vec![
        "revision_id UInt32".to_owned(),
        "tile_id UInt32".to_owned(),
        "cell_ids Array(UInt32)".to_owned(),
        "lead_hour UInt16".to_owned(),
    ];
    for name in &names {
        columns.push(format!("{name} Array(UInt128) CODEC(Delta(2),ZSTD(1))"));
    }
    db.command(&format!("CREATE TABLE {DATABASE}.{table} ({}) ENGINE=MergeTree PARTITION BY (lead_hour,intDiv(tile_id,128)) ORDER BY tile_id SETTINGS index_granularity=1,index_granularity_bytes=0,old_parts_lifetime=60,min_compress_block_size=32768,max_compress_block_size=65536",columns.join(",")))?;
    let mut merges = CandidateMerges::pause(&db, table)?;
    m["storage"] = json!({"layout":"cell_tiles_v1","axes":"level_lead","encoding":crate::dictionary::ENCODING,"dictionary":dictionary_info,"index_type":"UInt16","tile_size":crate::packed::CELLS,"words_per_slot":crate::packed::GROUPS,"packing":"uint16x8_le","element_type":"UInt128","codec":"delta","delta_bytes":2,"compression":"ZSTD(1)","index_granularity":1,"writer":"rust_native","partitioned_lead":true,"lead_hour":partition_lead});
    atomic(path.join("metadata.json"), &m)?;
    let storage_rows = nc.div_ceil(crate::packed::CELLS);
    let batch = match m["model"].as_str() {
        Some("hrrr") => 16384,
        Some("rrfs") => 8192,
        _ => 4096,
    };
    #[derive(Default)]
    struct Times {
        read: f64,
        encode: f64,
        insert: f64,
        finite: f64,
        allocator_purge: f64,
        allocator_purges: usize,
        cache_hint_failures: u64,
    }
    let next = AtomicUsize::new(0);
    let completed = AtomicUsize::new(0);
    let completed_batches = AtomicUsize::new(0);
    let cancelled = AtomicBool::new(false);
    let workers = workers.min(nc.div_ceil(batch));
    let max_active_inserts = workers.min(4);
    let insert_gate = InsertGate {
        available: Mutex::new(max_active_inserts),
        ready: Condvar::new(),
    };
    let readers = workers.min(2);
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
                    let mut code_input = vec![vec![0u16; batch * nl * nf]];
                    let mut code_ground = vec![vec![0u16; batch * 2]];
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
                        let body = {
                            let t = Instant::now();
                            for (array, buffer) in profiles.iter().zip(&mut code_input) {
                                times.cache_hint_failures += array.read_codes_aligned(
                                    start,
                                    c,
                                    &mut buffer[..c * nl * nf],
                                    &mut scratch,
                                )?;
                            }
                            for (array, buffer) in surfaces.iter().zip(&mut code_ground) {
                                times.cache_hint_failures += array.read_codes_aligned(
                                    start,
                                    c,
                                    &mut buffer[..c * 2],
                                    &mut scratch,
                                )?;
                            }
                            times.read += t.elapsed().as_secs_f64();
                            for &i in sample_cells.range(start..start + c) {
                                let local = i - start;
                                let sample = StagedSample {
                                    profiles: code_input
                                        .iter()
                                        .flat_map(|lead| {
                                            (0..nl * nf).map(move |slot| {
                                                f32::from_bits(u32::from(
                                                    lead[crate::packed::slot_offset(
                                                        c,
                                                        nl * nf,
                                                        local,
                                                        slot,
                                                    )],
                                                ))
                                            })
                                        })
                                        .collect(),
                                    surfaces: code_ground
                                        .iter()
                                        .flat_map(|lead| {
                                            (0..2).map(move |slot| {
                                                f32::from_bits(u32::from(
                                                    lead[crate::packed::slot_offset(
                                                        c, 2, local, slot,
                                                    )],
                                                ))
                                            })
                                        })
                                        .collect(),
                                };
                                stage_samples
                                    .lock()
                                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                                    .insert(i, sample);
                            }
                            let t = Instant::now();
                            let body = crate::packed::encode_lead(
                                names,
                                widths,
                                &cell_ids[start..start + c],
                                start,
                                &crate::packed::LeadInput {
                                    profiles: &code_input[0],
                                    surfaces: &code_ground[0],
                                    levels: nl,
                                    fields: nf,
                                    cells: c,
                                },
                            )?;
                            times.encode += t.elapsed().as_secs_f64();
                            body
                        };
                        let body = crate::packed::append_lead_column(
                            body,
                            c.div_ceil(crate::packed::CELLS),
                            partition_lead,
                        )?;
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
            let (
                rx,
                db,
                cancelled,
                completed,
                completed_batches,
                insert_gate,
                model,
                m,
                dictionary_limits,
            ) = (
                rx.clone(),
                &db,
                &cancelled,
                &completed,
                &completed_batches,
                &insert_gate,
                &m["model"],
                &m,
                &dictionary_limits,
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
                        {
                            let _permit = insert_gate.enter();
                            if cancelled.load(Ordering::Relaxed) {continue;}
                            db.insert(table,body)?;
                        }
                        times.insert += t.elapsed().as_secs_f64();
                        let t=Instant::now();
                        // Each private, disjoint range is checked from the
                        // database immediately, while its new parts are hot.
                        validate_inserted_batch(db,table,m,(start/crate::packed::CELLS) as u32,((start+c-1)/crate::packed::CELLS) as u32,c,dictionary_limits)?;
                        times.finite+=t.elapsed().as_secs_f64();
                        let done = completed.fetch_add(c,Ordering::Relaxed)+c;
                        // Large Native blocks leave allocator pages resident after
                        // their queries finish. Release unused pages periodically
                        // so background logging retains server memory headroom.
                        let batches = completed_batches.fetch_add(1,Ordering::Relaxed)+1;
                        if batches.is_multiple_of(4) {
                            let purge = Instant::now();
                            db.command("SYSTEM JEMALLOC PURGE")?;
                            times.allocator_purge += purge.elapsed().as_secs_f64();
                            times.allocator_purges += 1;
                        }
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
                    sum.encode += t.encode;
                    sum.insert += t.insert;
                    sum.finite += t.finite;
                    sum.allocator_purge += t.allocator_purge;
                    sum.allocator_purges += t.allocator_purges;
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
    let allocator_purge_work_seconds = times.allocator_purge;
    let allocator_purge_batches = times.allocator_purges;
    let mut times = json!({"read_seconds":times.read,"encode_seconds":times.encode,"insert_seconds":times.insert,"finite_validation_work_seconds":times.finite,"finite_validation_batches":nc.div_ceil(batch),"finite_validation_rows":nc,"total_insert_seconds":total_insert_seconds,"validation_seconds":t.elapsed().as_secs_f64(),"total_seconds":started.elapsed().as_secs_f64(),"storage_rows":nc,"logical_rows":nc*nt,"workers":workers,"readers":readers,"queued_batches":readers,"reader":reader,"pipeline":"bounded_readers_writers_validated_v4","candidate_merges_resumed":true,"input_cache_hint_failures":times.cache_hint_failures,"input_cache_policy":if prefetch {"prefetch_inside_load_then_discard_consumed_pages_linux_fadvise"}else{"discard_consumed_pages_linux_fadvise"},"input_prefetch_seconds":prefetch_seconds,"input_prefetch_bytes":if prefetch {input_bytes}else{0},"memory_available_before_prefetch_bytes":memory_available});
    times["storage_rows"] = json!(storage_rows);
    times["finite_validation_rows"] = json!(storage_rows);
    times["finite_validation_cells"] = json!(nc);
    times["allocator_purge_work_seconds"] = json!(allocator_purge_work_seconds);
    times["allocator_purge_batches"] = json!(allocator_purge_batches);
    times["max_active_inserts"] = json!(max_active_inserts);
    times["packing_pipeline"] = json!("direct_u16_stage_tiles");
    atomic(path.join("load-timings.json"), &times)?;
    Ok(evidence)
}
pub fn validate(path: &Path, table: &str, database: &str, staged: bool) -> Result<Value> {
    let scope = path
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "revisions"))
        .and_then(Path::parent);
    if let Some(root) = scope {
        let _guard = Lock::acquire(root, "catalog", false, false)?;
        let c = catalog(root)?;
        let name = path.file_name().context("Revision")?.to_string_lossy();
        if crate::runs::references(root, &c, true)?
            .iter()
            .any(|e| e.revision == name)
        {
            // Audit immutable published descriptors in a private workspace.
            // Their existing marker remains readable until its atomic refresh.
            let audit = root
                .join("audits")
                .join(uuid::Uuid::new_v4().simple().to_string());
            fs::create_dir_all(&audit)?;
            let result = (|| -> Result<Value> {
                for name in ["metadata.json", "cells.bin.zst", "values.dict.zst"] {
                    let src = path.join(name);
                    if src.exists() {
                        fs::hard_link(src, audit.join(name))?;
                    }
                }
                validate_impl(&audit, table, database, staged, None, None)
            })();
            let cleanup = fs::remove_dir_all(&audit);
            let evidence = result?;
            cleanup?;
            atomic(path.join("validated.json"), &evidence)?;
            return Ok(evidence);
        }
        return validate_impl(path, table, database, staged, None, None);
    }
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
    if partitioned(&m) {
        let e = Entry {
            revision: path
                .file_name()
                .context("Revision")?
                .to_string_lossy()
                .into_owned(),
            database: database.to_owned(),
            table: table.to_owned(),
            owned: true,
            retired_at: None,
        };
        let evidence = crate::runs::validate_manifest(crate::runs::root_for(path)?, &e, &m, true)?;
        atomic(marker, &evidence)?;
        return Ok(evidence);
    }
    anyhow::ensure!(
        crate::packed::valid_layout(&m),
        "Unsupported hourly tile layout"
    );
    let lead = u16::try_from(
        m["storage"]["lead_hour"]
            .as_u64()
            .context("Missing lead partition")?,
    )?;
    anyhow::ensure!(m["leads"] == json!([lead]), "Partition lead differs");
    let from = format!("(SELECT * FROM {database}.{table} WHERE lead_hour={lead})");
    let cell_ids = ids(path)?;
    let nc = cell_ids.len();
    anyhow::ensure!(
        m["cells"].as_u64() == Some(nc as u64),
        "Metadata cell count differs from index"
    );
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let db = Db::new()?;
    let storage_rows = db.scalar(&format!("SELECT count() FROM {from}"))?;
    anyhow::ensure!(
        storage_rows == nc.div_ceil(crate::packed::CELLS) as u64,
        "Wrong physical row count"
    );
    crate::packed::verify_cells_scoped(
        &db,
        database,
        table,
        &cell_ids,
        crate::packed::CELLS,
        Some(lead),
    )?;
    let dictionary = crate::dictionary::Dictionary::load(path, &m)?;
    let mut checks = vec![
        "revision_id != 1".to_owned(),
        format!("lead_hour!={lead}"),
        tile_cell_count_check(&m)?,
    ];
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
    for (name, expected) in [
        ("tile_id", "UInt32"),
        ("cell_ids", "Array(UInt32)"),
        ("lead_hour", "UInt16"),
        ("revision_id", "UInt32"),
    ] {
        anyhow::ensure!(
            types.get(name).map(String::as_str) == Some(expected),
            "Wrong physical type {name}"
        );
    }
    for (f, name) in names.iter().enumerate() {
        ident(name)?;
        anyhow::ensure!(
            types.get(name).map(String::as_str) == Some("Array(UInt128)"),
            "Wrong physical type {name}"
        );
        checks.push(crate::packed::array_check(
            name,
            if f < nf { nl } else { 1 },
            &dictionary.limits(f),
            crate::packed::GROUPS,
        ));
    }
    if let Some(checked) = checked_rows {
        anyhow::ensure!(
            checked == nc && preloaded.is_some(),
            "Incomplete in-memory range validation proof"
        );
    } else {
        // Audit one packed column at a time to bound memory after background merges.
        for check in checks {
            anyhow::ensure!(db.scalar(&format!("SELECT count() FROM {from} WHERE {check} SETTINGS max_threads=2,max_block_size=1,compile_expressions=1,min_count_to_compile_expression=0"))?==0,"Malformed/non-finite database values");
        }
    }
    let samples = sample_indices(nc);
    if let Some(cached) = preloaded {
        anyhow::ensure!(
            cached.keys().copied().collect::<BTreeSet<_>>() == samples
                && cached
                    .values()
                    .all(|v| v.profiles.len() == nl * nf && v.surfaces.len() == 2),
            "Incomplete staged verification samples"
        );
    }
    let books = BTreeMap::from([(lead, &dictionary)]);
    for &i in &samples {
        let (rows, _) = db.profile_leads(database, table, &m, cell_ids[i], i, &books)?;
        if staged {
            let mut pv = vec![0f32; nl * nf];
            let mut gv = [0f32; 2];
            if let Some(cached) = preloaded {
                pv.copy_from_slice(&cached[&i].profiles);
                gv.copy_from_slice(&cached[&i].surfaces);
            } else {
                let p = Array::open_as(
                    &path.join(format!("lead-{lead:03}")),
                    &[nc, nl, nf],
                    "uint16_le",
                )?;
                let g = Array::open_as(
                    &path.join(format!("surface-{lead:03}")),
                    &[nc, 2],
                    "uint16_le",
                )?;
                p.read_into(i * nl * nf, &mut pv)?;
                g.read_into(i * 2, &mut gv)?;
            }
            for f in 0..nf {
                let levels = rows[0][f + 1]
                    .as_array()
                    .context("Missing pressure values")?;
                anyhow::ensure!(levels.len() == nl, "Incorrect pressure length");
                for (l, value) in levels.iter().enumerate() {
                    let got = value.as_f64().context("Null profile")? as f32;
                    anyhow::ensure!(
                        got.to_bits() == dictionary.value(f, l, pv[l * nf + f].to_bits())?,
                        "Staged profile mismatch"
                    );
                }
            }
            for (f, value) in gv.iter().enumerate() {
                let got = rows[0][1 + nf + f].as_f64().context("Null surface")? as f32;
                anyhow::ensure!(
                    got.to_bits() == dictionary.value(nf + f, 0, value.to_bits())?,
                    "Staged surface mismatch"
                );
            }
        }
    }
    let evidence = json!({"rows":nc,"storage_rows":storage_rows,"table":table,"database":database,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"samples":samples.len(),"staged_comparison":staged,"staged_sample_origin":if preloaded.is_some(){"immutable_ingestion_buffers"}else{"staging_files"},"full_values_checked":true,"finite_validation_mode":if checked_rows.is_some(){"each_inserted_cell_range"}else{"whole_table_scan"},"storage":m["storage"],"validated_at":now(),"palette_values_checked":dictionary.entry_count()?,"logical_weather_values":nc*(nf*nl+2),"stored_indices_checked":nc.div_ceil(crate::packed::CELLS)*crate::packed::CELLS*(nf*nl+2),"decoded_finite_proof":"every_palette_value_finite_and_every_stored_index_in_its_slot_bounds"});
    atomic(marker, &evidence)?;
    Ok(evidence)
}
type HashTypes = std::collections::HashMap<String, String>;
pub fn collect(root: &Path, grace: f64) -> Result<()> {
    let _l = Lock::acquire(root, "catalog", true, false)?;
    let mut c = catalog(root)?;
    let mut protected = c.clone();
    protected
        .retired
        .retain(|e| now() - e.retired_at.unwrap_or_else(now) < grace);
    let references = crate::runs::references(root, &protected, true)?;
    let keep = references
        .iter()
        .map(|e| e.revision.clone())
        .collect::<std::collections::HashSet<_>>();
    let tables = references
        .iter()
        .map(|e| (e.database.clone(), e.table.clone()))
        .collect::<std::collections::HashSet<_>>();
    let db = Db::new()?;
    let mut pending = Vec::new();
    let mut dropped = std::collections::HashSet::new();
    for e in &c.retired {
        if keep.contains(&e.revision) {
            pending.push(e.clone());
            continue;
        }
        if e.owned
            && !tables.contains(&(e.database.clone(), e.table.clone()))
            && dropped.insert((e.database.clone(), e.table.clone()))
        {
            anyhow::ensure!(
                e.database == DATABASE,
                "Retention cannot delete tables outside its database"
            );
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
/// Caller holds update.lock: publication must not change this protection snapshot.
pub fn sweep_unreferenced(root: &Path) -> Result<usize> {
    let references = crate::runs::references(root, &catalog(root)?, true)?;
    let mut examined = 0;
    for file in fs::read_dir(root.join("revisions"))? {
        let path = file?.path();
        if path.is_dir() {
            discard_with_references(&path, &references)?;
            examined += 1;
        }
    }
    Ok(examined)
}
pub fn discard(root: &Path, path: &Path) -> Result<()> {
    let references = crate::runs::references(root, &catalog(root)?, true)?;
    discard_with_references(path, &references)
}
fn discard_with_references(path: &Path, references: &[Entry]) -> Result<()> {
    let name = path
        .file_name()
        .context("No revision name")?
        .to_string_lossy();
    if references.iter().any(|e| e.revision == name) {
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
    if e.owned
        && !references
            .iter()
            .any(|other| other.database == e.database && other.table == e.table)
    {
        anyhow::ensure!(
            e.database == DATABASE,
            "Discard cannot delete tables outside its database"
        );
        Db::new()?.command(&format!(
            "DROP TABLE IF EXISTS {}.{} SYNC",
            ident(&e.database)?,
            ident(&e.table)?
        ))?;
    }
    clean_workspace(path)?;
    for name in [
        "cells.bin.zst",
        "values.dict.zst",
        "native-clouds.stage.zst",
        "native-clouds.stage.json",
        crate::cloud_coverage::DATA,
        crate::cloud_coverage::INDEX,
        "cloud-anchors.stage.zst",
        "cloud-anchors.stage.json",
    ] {
        let cells = path.join(name);
        if cells.exists() {
            fs::remove_file(cells)?;
        }
    }
    atomic(path.join("discarded.json"), &json!({"time":now()}))
}
