use crate::{db::Db, pack, stage::Array, state::*};
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
fn array_check(name: &str, width: usize, bit: bool) -> String {
    let invalid = if bit {
        "bitAnd(x,2139095040)=2139095040"
    } else {
        "NOT isFinite(x)"
    };
    format!("length({name})!={width} OR arrayExists(x -> {invalid},{name})")
}
fn dictionary_check(name: &str, width: usize, limits: &[u32]) -> String {
    let bounds = limits
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    format!("length({name})!={width} OR arrayExists((code,bound) -> code>=bound,{name},CAST([{bounds}] AS Array(UInt32)))")
}
pub(crate) fn validate_inserted_batch(
    db: &Db,
    table: &str,
    m: &Value,
    lo: u32,
    hi: u32,
    rows: usize,
) -> Result<()> {
    validate_inserted_batch_with(db, table, m, lo, hi, rows, None)
}
fn validate_inserted_batch_with(
    db: &Db,
    table: &str,
    m: &Value,
    lo: u32,
    hi: u32,
    rows: usize,
    limits: Option<&[Vec<u32>]>,
) -> Result<()> {
    ident(table)?;
    let tiled = crate::packed::tiled(m);
    let tile_cells = if tiled {
        crate::packed::tile_cells(m)?
    } else {
        1
    };
    anyhow::ensure!(
        (m["storage"]["layout"] == "cell_profile_v1" || crate::packed::valid_layout(m))
            && (m["storage"]["encoding"] == "float32_bits" || crate::dictionary::encoded(m)),
        "Batch validation needs the native bit layout"
    );
    let nt = m["leads"].as_array().context("Leads")?.len();
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let mut checks = vec!["revision_id != 1".to_owned()];
    if tiled {
        checks.push(tile_cell_count_check(m)?);
    }
    for (f, name) in fields(m).iter().enumerate() {
        ident(name)?;
        let width = if f < nf { nl * nt } else { nt };
        checks.push(if tiled {
            crate::packed::array_check(
                name,
                width,
                &limits.context("Missing dictionary bounds")?[f],
                tile_cells / crate::packed::LANES,
            )
        } else if crate::dictionary::encoded(m) {
            dictionary_check(
                name,
                width,
                &limits.context("Missing dictionary bounds")?[f],
            )
        } else {
            array_check(name, width, true)
        });
    }
    let rows = if tiled {
        rows.div_ceil(tile_cells)
    } else {
        rows
    };
    let key = if tiled { "tile_id" } else { "cell_id" };
    let block = if tiled { 1024 / tile_cells } else { 1024 };
    anyhow::ensure!(db.scalar(&format!("SELECT toUInt8(count()={rows} AND min({key})={lo} AND max({key})={hi} AND countIf({})=0) FROM {DATABASE}.{table} WHERE {key}>={lo} AND {key}<={hi} SETTINGS max_threads=2,max_block_size={block},compile_expressions=1,min_count_to_compile_expression=0",checks.join(" OR ")))?==1,"Malformed/non-finite or incomplete inserted range {lo}..{hi}");
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
fn encode(
    names: &[String],
    columns: &[&[f32]],
    widths: &[usize],
    ids: &[u32],
    index16: bool,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        names.len() == columns.len()
            && widths.len() == names.len()
            && columns
                .iter()
                .zip(widths)
                .all(|(column, width)| ids.len().checked_mul(*width) == Some(column.len())),
        "Native input column dimensions"
    );
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
            if index16 { 2 } else { 1 },
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
    load_with_layout(root, path, table, workers, false)
}
pub fn load_with_layout(
    root: &Path,
    path: &Path,
    table: &str,
    workers: Option<usize>,
    packed: bool,
) -> Result<Value> {
    load_impl(root, path, table, workers, packed, None)
}
pub fn load_lead(root: &Path, path: &Path, table: &str, lead: u16) -> Result<Value> {
    load_impl(root, path, table, None, true, Some(lead))
}
fn load_impl(
    root: &Path,
    path: &Path,
    table: &str,
    workers: Option<usize>,
    packed: bool,
    partition_lead: Option<u16>,
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
    anyhow::ensure!(
        partition_lead.is_none_or(|h| leads.as_slice() == [json!(h)]),
        "A partition must contain exactly its forecast hour"
    );
    anyhow::ensure!(nt > 0 && nl > 0 && nf > 0, "Empty forecast axes");
    anyhow::ensure!(
        m["surface_variables"].as_array().map(Vec::len) == Some(2),
        "Native staging requires two surface fields"
    );
    let mut profiles = vec![];
    let mut surfaces = vec![];
    let dictionary = crate::dictionary::Dictionary::load(path, &m)?;
    let dtype = if dictionary.is_some() {
        "uint32_le"
    } else {
        "float32_le"
    };
    let dictionary_info = if crate::dictionary::staged(&m) {
        m["staging"]["dictionary"].clone()
    } else {
        m["storage"]["dictionary"].clone()
    };
    let dictionary_limits = dictionary.as_ref().map(|d| {
        (0..fields(&m).len())
            .map(|f| d.limits(f))
            .collect::<Vec<_>>()
    });
    let index16 = dictionary_limits
        .as_ref()
        .is_some_and(|fields| fields.iter().flatten().all(|n| *n <= 65536));
    let index_type = if index16 { "UInt16" } else { "UInt32" };
    anyhow::ensure!(
        !packed || index16,
        "Packed loading requires UInt16 dictionaries"
    );
    let regional = m["model"].as_str().is_some_and(grid_relative);
    let tiled = index16 && (regional || packed);
    let storage_rows = if tiled {
        nc.div_ceil(crate::packed::CELLS)
    } else {
        nc
    };
    let workers = workers.unwrap_or(if regional {
        if tiled {
            6
        } else {
            4
        }
    } else {
        1
    });
    anyhow::ensure!(
        (1..=6).contains(&workers),
        "Loader workers must be 1 through 6"
    );
    for l in leads {
        let lead = l.as_u64().context("Invalid lead")?;
        profiles.push(Array::open_as(
            &path.join(format!("lead-{lead:03}.npy")),
            &[nc, nl, nf],
            dtype,
        )?);
        surfaces.push(Array::open_as(
            &path.join(format!("surface-{lead:03}.npy")),
            &[nc, 2],
            dtype,
        )?);
    }
    let reader = profiles[0].format_name();
    let prefetch_start = Instant::now();
    let input_bytes = profiles
        .iter()
        .chain(&surfaces)
        .map(Array::byte_len)
        .sum::<Result<u64>>()?;
    let memory_available = fs::read_to_string("/proc/meminfo").ok().and_then(|text| {
        text.lines().find_map(|line| {
            line.strip_prefix("MemAvailable:")
                .and_then(|rest| rest.split_whitespace().next()?.parse::<u64>().ok())
                .map(|kb| kb * 1024)
        })
    });
    // Reserve the 12 GiB database cap plus a 4 GiB client and 2 GiB margin.
    // Large/raw inputs and smaller hosts retain the original streaming path.
    let prefetch = index16
        && input_bytes <= 8 * 1024u64.pow(3)
        && memory_available.is_some_and(|available| available >= input_bytes + 18 * 1024u64.pow(3));
    if prefetch {
        let mut buffer = vec![0u8; 1024 * 1024];
        for array in profiles.iter().chain(&surfaces) {
            array.prefetch(&mut buffer)?;
        }
    }
    let prefetch_seconds = prefetch_start.elapsed().as_secs_f64();
    let names = fields(&m);
    for n in &names {
        ident(n)?;
    }
    let db = Db::new()?;
    db.command(&format!("CREATE DATABASE IF NOT EXISTS {DATABASE}"))?;
    if tiled {
        db.command("SYSTEM JEMALLOC PURGE")?;
    }
    let mut columns = vec![
        "revision_id UInt32".to_owned(),
        if tiled {
            "tile_id UInt32"
        } else {
            "cell_id UInt32"
        }
        .to_owned(),
    ];
    if tiled {
        columns.push("cell_ids Array(UInt32)".to_owned());
    }
    if partition_lead.is_some() {
        columns.push("lead_hour UInt16".to_owned());
    }
    let compression = if regional && !index16 {
        "ZSTD(3)"
    } else {
        "ZSTD(1)"
    };
    for f in &names {
        columns.push(if tiled {
            format!("{f} Array(UInt128) CODEC(Delta(2),{compression})")
        } else {
            format!("{f} Array({index_type}) CODEC(T64,{compression})")
        });
    }
    let key = if tiled { "tile_id" } else { "cell_id" };
    let partition = if tiled {
        32768 / crate::packed::CELLS
    } else {
        32768
    };
    let granularity = if tiled { 1 } else { 4 };
    let partition_key = if partition_lead.is_some() {
        format!("(lead_hour,intDiv({key},{partition}))")
    } else {
        format!("(revision_id,intDiv({key},{partition}))")
    };
    let compression_block = if partition_lead.is_some() {
        ",min_compress_block_size=32768,max_compress_block_size=65536"
    } else {
        ""
    };
    db.command(&format!("CREATE TABLE {DATABASE}.{table} ({}) ENGINE=MergeTree PARTITION BY {partition_key} ORDER BY {key} SETTINGS index_granularity={granularity},index_granularity_bytes=0,old_parts_lifetime=60{compression_block}",columns.join(",")))?;
    let mut merges = CandidateMerges::pause(&db, table)?;
    m["storage"] = json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":"float32_bits","codec":"t64","compression":compression,"index_granularity":4,"writer":"rust_native"});
    if dictionary.is_some() {
        m["storage"]["encoding"] = json!(crate::dictionary::ENCODING);
        m["storage"]["dictionary"] = dictionary_info;
        m["storage"]["index_type"] = json!(index_type);
    }
    if tiled {
        m["storage"]["layout"] = json!("cell_tiles_v1");
        m["storage"]["tile_size"] = json!(crate::packed::CELLS);
        m["storage"]["words_per_slot"] = json!(crate::packed::GROUPS);
        m["storage"]["packing"] = json!("uint16x8_le");
        m["storage"]["element_type"] = json!("UInt128");
        m["storage"]["codec"] = json!("delta");
        m["storage"]["delta_bytes"] = json!(2);
        m["storage"]["index_granularity"] = json!(granularity);
    }
    if let Some(lead) = partition_lead {
        m["storage"]["partitioned_lead"] = json!(true);
        m["storage"]["lead_hour"] = json!(lead);
    }
    atomic(path.join("metadata.json"), &m)?;
    let batch = match m["model"].as_str() {
        Some("gfs") if tiled => 4096,
        Some("gfs") => 2048,
        Some("hrrr") if tiled => 16384,
        Some("hrrr") if index16 => 8192,
        Some("hrrr") => 4096,
        // 85 leads versus HRRR's 49: keep each Native block below the
        // measured HRRR byte budget and retain 4,096-cell frame alignment.
        Some("rrfs") if tiled => 8192,
        Some("rrfs") if index16 => 4096,
        Some("rrfs") => 2048,
        _ => 4096,
    };
    #[derive(Default)]
    struct Times {
        read: f64,
        pack: f64,
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
    let max_active_inserts = workers.min(if tiled { 4 } else { workers });
    let insert_gate = InsertGate {
        available: Mutex::new(max_active_inserts),
        ready: Condvar::new(),
    };
    let readers = workers.min(if tiled { 2 } else { 3 });
    let direct = tiled && profiles.iter().chain(&surfaces).all(Array::short_codes);
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
                    let mut input: Vec<_> = (0..nt)
                        .map(|_| vec![0f32; if direct { 0 } else { batch * nl * nf }])
                        .collect();
                    let mut ground: Vec<_> = (0..nt)
                        .map(|_| vec![0f32; if direct { 0 } else { batch * 2 }])
                        .collect();
                    let mut code_input: Vec<_> = (0..nt)
                        .map(|_| vec![0u16; if direct { batch * nl * nf } else { 0 }])
                        .collect();
                    let mut code_ground: Vec<_> = (0..nt)
                        .map(|_| vec![0u16; if direct { batch * 2 } else { 0 }])
                        .collect();
                    let mut p = vec![0f32; if direct { 0 } else { batch * nl * nf * nt }];
                    let mut g = vec![0f32; if direct { 0 } else { batch * 2 * nt }];
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
                        let body = if direct {
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
                            let body = crate::packed::encode_leads(
                                names,
                                widths,
                                &cell_ids[start..start + c],
                                start,
                                &crate::packed::LeadInput {
                                    profiles: &code_input,
                                    surfaces: &code_ground,
                                    levels: nl,
                                    fields: nf,
                                    cells: c,
                                },
                            )?;
                            times.encode += t.elapsed().as_secs_f64();
                            body
                        } else {
                            let t = Instant::now();
                            for (a, buffer) in profiles.iter().zip(&mut input) {
                                times.cache_hint_failures += a.read_into_with(
                                    start * nl * nf,
                                    &mut buffer[..c * nl * nf],
                                    &mut scratch,
                                )?;
                            }
                            for (a, buffer) in surfaces.iter().zip(&mut ground) {
                                times.cache_hint_failures += a.read_into_with(
                                    start * 2,
                                    &mut buffer[..c * 2],
                                    &mut scratch,
                                )?;
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
                                            v[local * nl * nf..(local + 1) * nl * nf]
                                                .iter()
                                                .copied()
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
                            let body = if tiled {
                                crate::packed::encode(
                                    names,
                                    &cols,
                                    widths,
                                    &cell_ids[start..start + c],
                                    start,
                                )?
                            } else {
                                encode(names, &cols, widths, &cell_ids[start..start + c], index16)?
                            };
                            times.encode += t.elapsed().as_secs_f64();
                            drop(cols);
                            body
                        };
                        let body = if let Some(lead) = partition_lead {
                            crate::packed::append_lead_column(
                                body,
                                c.div_ceil(crate::packed::CELLS),
                                lead,
                            )?
                        } else {
                            body
                        };
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
                cell_ids,
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
                &cell_ids,
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
                        if dictionary_limits.is_some(){let (lo,hi)=if tiled {((start/crate::packed::CELLS) as u32,((start+c-1)/crate::packed::CELLS) as u32)}else{(cell_ids[start],cell_ids[start+c-1])};validate_inserted_batch_with(db,table,m,lo,hi,c,dictionary_limits.as_deref())?;}else{validate_inserted_batch(db,table,m,cell_ids[start],cell_ids[start+c-1],c)?;}
                        times.finite+=t.elapsed().as_secs_f64();
                        let done = completed.fetch_add(c,Ordering::Relaxed)+c;
                        // Large Native blocks leave allocator pages resident after
                        // their queries finish. Release unused pages periodically
                        // so background logging retains server memory headroom.
                        let batches = completed_batches.fetch_add(1,Ordering::Relaxed)+1;
                        if tiled && batches.is_multiple_of(4) {
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
                    sum.pack += t.pack;
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
    let mut times = json!({"read_seconds":times.read,"pack_seconds":times.pack,"encode_seconds":times.encode,"insert_seconds":times.insert,"finite_validation_work_seconds":times.finite,"finite_validation_batches":nc.div_ceil(batch),"finite_validation_rows":nc,"total_insert_seconds":total_insert_seconds,"validation_seconds":t.elapsed().as_secs_f64(),"total_seconds":started.elapsed().as_secs_f64(),"storage_rows":nc,"logical_rows":nc*nt,"workers":workers,"readers":readers,"queued_batches":readers,"reader":reader,"pipeline":"bounded_readers_writers_validated_v4","candidate_merges_resumed":true,"input_cache_hint_failures":times.cache_hint_failures,"input_cache_policy":if prefetch {"prefetch_inside_load_then_discard_consumed_pages_linux_fadvise"}else{"discard_consumed_pages_linux_fadvise"},"input_prefetch_seconds":prefetch_seconds,"input_prefetch_bytes":if prefetch {input_bytes}else{0},"memory_available_before_prefetch_bytes":memory_available});
    times["storage_rows"] = json!(storage_rows);
    times["finite_validation_rows"] = json!(storage_rows);
    times["finite_validation_cells"] = json!(nc);
    times["allocator_purge_work_seconds"] = json!(allocator_purge_work_seconds);
    times["allocator_purge_batches"] = json!(allocator_purge_batches);
    times["max_active_inserts"] = json!(max_active_inserts);
    times["packing_pipeline"] = json!(if direct {
        "direct_u16_stage_tiles"
    } else {
        "transposed_f32_buffers"
    });
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
                for name in [
                    "metadata.json",
                    "cells.bin.zst",
                    "cells.npz",
                    "values.dict.zst",
                ] {
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
    let partition_lead = m["storage"]["lead_hour"]
        .as_u64()
        .map(|h| u16::try_from(h).context("Invalid lead partition"))
        .transpose()?;
    let from = if let Some(lead) = partition_lead {
        format!("(SELECT * FROM {database}.{table} WHERE lead_hour={lead})")
    } else {
        format!("{database}.{table}")
    };
    let cell_ids = ids(path)?;
    let nc = cell_ids.len();
    anyhow::ensure!(
        m["cells"].as_u64() == Some(nc as u64),
        "Metadata cell count differs from index"
    );
    let nt = m["leads"].as_array().context("Leads")?.len();
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let tiled = crate::packed::tiled(&m);
    anyhow::ensure!(
        !tiled || crate::packed::valid_layout(&m),
        "Unknown packed layout"
    );
    let tile_cells = if tiled {
        crate::packed::tile_cells(&m)?
    } else {
        1
    };
    let profile = m["storage"]["layout"] == "cell_profile_v1" || tiled;
    let db = Db::new()?;
    let storage_rows = db.scalar(&format!("SELECT count() FROM {from}"))?;
    anyhow::ensure!(
        storage_rows
            == if tiled {
                nc.div_ceil(tile_cells)
            } else if profile {
                nc
            } else {
                nc * nt
            } as u64,
        "Wrong physical row count"
    );
    if tiled {
        crate::packed::verify_cells_scoped(
            &db,
            database,
            table,
            &cell_ids,
            tile_cells,
            partition_lead,
        )?;
    } else {
        let actual = db.bytes(&format!(
            "SELECT {}cell_id FROM {from} {}ORDER BY cell_id FORMAT RowBinary",
            if profile { "" } else { "DISTINCT " },
            ""
        ))?;
        let expected: Vec<u8> = cell_ids.iter().flat_map(|v| v.to_le_bytes()).collect();
        anyhow::ensure!(
            actual == expected,
            "Missing/duplicated/substituted cell ids"
        );
    }
    let mut checks = vec!["revision_id != 1".to_owned()];
    if let Some(lead) = partition_lead {
        anyhow::ensure!(m["leads"] == json!([lead]), "Partition lead differs");
        checks.push(format!("lead_hour!={lead}"));
    }
    if tiled {
        checks.push(tile_cell_count_check(&m)?);
    }
    let bit = m["storage"]["encoding"] == "float32_bits";
    let dictionary = crate::dictionary::Dictionary::load(path, &m)?;
    let coded = crate::dictionary::encoded(&m);
    anyhow::ensure!(!coded || dictionary.is_some(), "Missing value dictionary");
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
    if tiled {
        anyhow::ensure!(
            types.get("tile_id").map(String::as_str) == Some("UInt32")
                && types.get("cell_ids").map(String::as_str) == Some("Array(UInt32)"),
            "Wrong packed key/ID type"
        );
    }
    if partition_lead.is_some() {
        anyhow::ensure!(
            types.get("lead_hour").map(String::as_str) == Some("UInt16"),
            "Wrong lead key type"
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
            let ty = if tiled {
                "Array(UInt128)"
            } else if coded && crate::dictionary::index_bytes(&m)? == 2 {
                "Array(UInt16)"
            } else if bit || coded {
                "Array(UInt32)"
            } else {
                "Array(Float32)"
            };
            anyhow::ensure!(
                types.get(name).map(String::as_str) == Some(ty),
                "Wrong physical type {name}"
            );
            checks.push(if tiled {
                crate::packed::array_check(
                    name,
                    width,
                    &dictionary.as_ref().unwrap().limits(f),
                    tile_cells / crate::packed::LANES,
                )
            } else if coded {
                dictionary_check(name, width, &dictionary.as_ref().unwrap().limits(f))
            } else {
                array_check(name, width, bit)
            });
        } else {
            checks.push(format!("NOT isFinite({name})"));
        }
    }
    if let Some(checked) = checked_rows {
        anyhow::ensure!(
            profile && (bit || coded) && checked == nc && preloaded.is_some(),
            "Incomplete in-memory range validation proof"
        );
    } else {
        // Every tile contains all leads/levels for 256 cells. Audit packed
        // columns separately, with bounded parallelism and a one-tile block:
        // reading all eight wide arrays together can exceed 2 GiB even when
        // the original parts passed, after background merges change buffers.
        let (threads, block) = if tiled { (2, 1) } else { (8, 1024) };
        let scans = if tiled {
            checks
        } else {
            vec![checks.join(" OR ")]
        };
        for check in scans {
            anyhow::ensure!(
                db.scalar(&format!(
                    "SELECT count() FROM {from} WHERE {check} SETTINGS max_threads={threads},max_block_size={block},compile_expressions=1,min_count_to_compile_expression=0"
                ))? == 0,
                "Malformed/non-finite database values"
            );
        }
    }
    if !profile {
        // A global 80-million-key set exceeds the 2 GiB query budget.
        for range in cell_ids[0] / 32768..=cell_ids[nc - 1] / 32768 {
            let lo = range * 32768;
            let hi = lo + 32768;
            let cells =
                cell_ids.partition_point(|id| *id < hi) - cell_ids.partition_point(|id| *id < lo);
            anyhow::ensure!(db.scalar(&format!("SELECT uniqExact((cell_id,lead_hour)) FROM {from} WHERE cell_id>={lo} AND cell_id<{hi}"))? == (cells * nt) as u64, "Duplicate/missing legacy cell/lead keys in range {range}");
        }
        let counts = db.json(&format!(
            "SELECT lead_hour,count() AS n FROM {from} GROUP BY lead_hour ORDER BY lead_hour"
        ))?;
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
        let (rows, _) =
            db.profile_with_dictionary(database, table, &m, cell_ids[i], dictionary.as_ref())?;
        if staged {
            for (t, lead) in m["leads"].as_array().unwrap().iter().enumerate() {
                let lead = lead.as_u64().unwrap();
                let mut pv = vec![0f32; nl * nf];
                let mut gv = [0f32; 2];
                if let Some(cached) = preloaded {
                    pv.copy_from_slice(&cached[&i].profiles[t * nl * nf..(t + 1) * nl * nf]);
                    gv.copy_from_slice(&cached[&i].surfaces[t * 2..(t + 1) * 2]);
                } else {
                    let dtype = if coded { "uint32_le" } else { "float32_le" };
                    let p = Array::open_as(
                        &path.join(format!("lead-{lead:03}.npy")),
                        &[nc, nl, nf],
                        dtype,
                    )?;
                    let g = Array::open_as(
                        &path.join(format!("surface-{lead:03}.npy")),
                        &[nc, 2],
                        dtype,
                    )?;
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
                            got.to_bits()
                                == if let Some(d) = &dictionary {
                                    d.value(f, l * nt + t, pv[l * nf + f].to_bits())?
                                } else {
                                    pv[l * nf + f].to_bits()
                                },
                            "Staged profile mismatch"
                        );
                    }
                }
                for f in 0..2 {
                    let got = rows[t][1 + nf + f].as_f64().context("Null surface")? as f32;
                    anyhow::ensure!(
                        got.to_bits()
                            == if let Some(d) = &dictionary {
                                d.value(nf + f, t, gv[f].to_bits())?
                            } else {
                                gv[f].to_bits()
                            },
                        "Staged surface mismatch"
                    );
                }
            }
        }
    }
    let mut evidence = json!({"rows":nc*nt,"storage_rows":storage_rows,"table":table,"database":database,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"samples":samples.len(),"staged_comparison":staged,"staged_sample_origin":if preloaded.is_some(){"immutable_ingestion_buffers"}else{"staging_files"},"full_values_checked":true,"finite_validation_mode":if checked_rows.is_some(){"each_inserted_cell_range"}else{"whole_table_scan"},"storage":m.get("storage"),"validated_at":now()});
    if let Some(dictionary) = &dictionary {
        evidence["palette_values_checked"] = json!(dictionary
            .fields
            .iter()
            .flatten()
            .map(Vec::len)
            .sum::<usize>());
        evidence["logical_weather_values"] = json!(nc * nt * (nf * nl + 2));
        evidence["stored_indices_checked"] = json!(
            if tiled {
                nc.div_ceil(tile_cells) * tile_cells
            } else {
                nc
            } * nt
                * (nf * nl + 2)
        );
        evidence["decoded_finite_proof"] =
            json!("every_palette_value_finite_and_every_stored_index_in_its_slot_bounds");
    }
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
pub fn discard(root: &Path, path: &Path) -> Result<()> {
    let c = catalog(root)?;
    let name = path
        .file_name()
        .context("No revision name")?
        .to_string_lossy();
    let references = crate::runs::references(root, &c, true)?;
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
    for name in ["cells.npz", "cells.bin.zst", "values.dict.zst"] {
        let cells = path.join(name);
        if cells.exists() {
            fs::remove_file(cells)?;
        }
    }
    atomic(path.join("discarded.json"), &json!({"time":now()}))
}
