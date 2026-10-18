use crate::{db::Db, pack, state::*};
use anyhow::{Context, Result};
use memmap2::{Mmap, MmapOptions};
use serde_json::{json, Value};
use std::{
    fs::{self, File},
    path::Path,
    sync::{mpsc, Arc},
    time::Instant,
};
struct Array {
    map: Mmap,
    offset: usize,
}
impl Array {
    fn open(p: &Path, expected: &[usize]) -> Result<Self> {
        let f = File::open(p)?;
        let map = unsafe { MmapOptions::new().map(&f)? };
        let (shape, dtype, offset) = parse_npy(&map)?;
        anyhow::ensure!(
            shape == expected
                && dtype == "<f4"
                && map.len() == offset + expected.iter().product::<usize>() * 4
                && offset % 4 == 0,
            "Invalid staged array {}",
            p.display()
        );
        Ok(Self { map, offset })
    }
    fn ptr(&self, start: usize) -> *const f32 {
        unsafe { self.map.as_ptr().add(self.offset + start * 4) as *const f32 }
    }
    fn value(&self, i: usize) -> f32 {
        f32::from_le_bytes(
            self.map[self.offset + i * 4..self.offset + (i + 1) * 4]
                .try_into()
                .unwrap(),
        )
    }
}
fn ids(path: &Path) -> Result<Vec<u32>> {
    let cells = load_npz(&path.join("cells.npz"))?;
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
    ident(table)?;
    let started = Instant::now();
    let mut m = read_json(path.join("metadata.json"))?;
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
    let names = fields(&m);
    for n in &names {
        ident(n)?;
    }
    let db = Db::new()?;
    db.command(&format!("CREATE DATABASE IF NOT EXISTS {DATABASE}"))?;
    let mut columns = vec!["revision_id UInt32".to_owned(), "cell_id UInt32".to_owned()];
    for f in &names {
        columns.push(format!("{f} Array(UInt32) CODEC(T64,ZSTD(1))"));
    }
    db.command(&format!("CREATE TABLE {DATABASE}.{table} ({}) ENGINE=MergeTree PARTITION BY (revision_id,intDiv(cell_id,32768)) ORDER BY cell_id SETTINGS index_granularity=4,index_granularity_bytes=0,old_parts_lifetime=60",columns.join(",")))?;
    m["storage"] = json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":"float32_bits","codec":"t64","index_granularity":4,"writer":"rust_native"});
    atomic(path.join("metadata.json"), &m)?;
    let batch = if m["model"] == "gfs" { 2048 } else { 4096 };
    let profiles = Arc::new(profiles);
    let surfaces = Arc::new(surfaces);
    let (tx, rx) = mpsc::sync_channel(1);
    let producer = std::thread::spawn(move || -> Result<f64> {
        let mut seconds = 0.;
        for start in (0..nc).step_by(batch) {
            let c = batch.min(nc - start);
            let time = Instant::now();
            let pp: Vec<_> = profiles.iter().map(|a| a.ptr(start * nl * nf)).collect();
            let gp: Vec<_> = surfaces.iter().map(|a| a.ptr(start * 2)).collect();
            let mut p = vec![0f32; c * nl * nf * nt];
            let mut g = vec![0f32; c * 2 * nt];
            let bad = unsafe {
                pack::pack_neon(pp.as_ptr(), p.as_mut_ptr(), c, nl, nf, nt)
                    | pack::pack_neon(gp.as_ptr(), g.as_mut_ptr(), c, 1, 2, nt)
            };
            anyhow::ensure!(bad == 0, "Non-finite staging");
            seconds += time.elapsed().as_secs_f64();
            if tx.send((start, c, p, g)).is_err() {
                return Ok(seconds);
            }
        }
        Ok(seconds)
    });
    let mut insert_seconds = 0.;
    let mut encode_seconds = 0.;
    let mut pack_wait = 0.;
    let insert_start = Instant::now();
    let result = (|| -> Result<()> {
        loop {
            let wait = Instant::now();
            let msg = rx.recv();
            pack_wait += wait.elapsed().as_secs_f64();
            let (start, c, p, g) = match msg {
                Ok(v) => v,
                Err(_) => break,
            };
            disk_guard(root, 0)?;
            let widths: Vec<_> = (0..names.len())
                .map(|f| if f < nf { nl * nt } else { nt })
                .collect();
            let t = Instant::now();
            let cols: Vec<&[f32]> = p
                .chunks_exact(c * nl * nt)
                .chain(g.chunks_exact(c * nt))
                .collect();
            let body = encode(&names, &cols, &widths, &cell_ids[start..start + c])?;
            encode_seconds += t.elapsed().as_secs_f64();
            drop(cols);
            drop(p);
            drop(g);
            let t = Instant::now();
            db.insert(table, body)?;
            insert_seconds += t.elapsed().as_secs_f64();
            if start % (batch * 10) == 0 {
                println!(
                    "{}",
                    json!({"phase":"load","model":m["model"],"cells":start+c,"total":nc})
                );
            }
        }
        Ok(())
    })();
    drop(rx);
    let pack_seconds = producer
        .join()
        .map_err(|_| anyhow::anyhow!("Packing thread panicked"))??;
    result?;
    let total_insert_seconds = insert_start.elapsed().as_secs_f64();
    let t = Instant::now();
    let evidence = validate(path, table, DATABASE, true)?;
    let times = json!({"pack_seconds":pack_seconds,"pack_wait_seconds":pack_wait,"encode_seconds":encode_seconds,"insert_seconds":insert_seconds,"total_insert_seconds":total_insert_seconds,"validation_seconds":t.elapsed().as_secs_f64(),"total_seconds":started.elapsed().as_secs_f64(),"storage_rows":nc,"logical_rows":nc*nt});
    atomic(path.join("load-timings.json"), &times)?;
    Ok(evidence)
}
pub fn validate(path: &Path, table: &str, database: &str, staged: bool) -> Result<Value> {
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
    let invalid = if bit {
        "bitAnd(x,2139095040)=2139095040"
    } else {
        "NOT isFinite(x)"
    };
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
            checks.push(format!(
                "length({name})!={width} OR arrayExists(x -> {invalid},{name})"
            ));
        } else {
            checks.push(format!("NOT isFinite({name})"));
        }
    }
    anyhow::ensure!(
        db.scalar(&format!(
            "SELECT count() FROM {database}.{table} WHERE {}",
            checks.join(" OR ")
        ))? == 0,
        "Malformed/non-finite database values"
    );
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
    let samples: std::collections::BTreeSet<usize> = std::iter::once(0)
        .chain(std::iter::once(nc - 1))
        .chain((1..9).map(|i| ((i as u64 * 104729) % nc as u64) as usize))
        .collect();
    for &i in &samples {
        let (rows, _) = db.profile(database, table, &m, cell_ids[i])?;
        if staged {
            for (t, lead) in m["leads"].as_array().unwrap().iter().enumerate() {
                let lead = lead.as_u64().unwrap();
                let p = Array::open(&path.join(format!("lead-{lead:03}.npy")), &[nc, nl, nf])?;
                let g = Array::open(&path.join(format!("surface-{lead:03}.npy")), &[nc, 2])?;
                for f in 0..nf {
                    let levels = rows[t][f + 1]
                        .as_array()
                        .context("Missing pressure values")?;
                    anyhow::ensure!(levels.len() == nl, "Incorrect pressure length");
                    for (l, value) in levels.iter().enumerate() {
                        let got = value.as_f64().context("Null profile")? as f32;
                        anyhow::ensure!(
                            got.to_bits() == p.value((i * nl + l) * nf + f).to_bits(),
                            "Staged profile mismatch"
                        );
                    }
                }
                for f in 0..2 {
                    let got = rows[t][1 + nf + f].as_f64().context("Null surface")? as f32;
                    anyhow::ensure!(
                        got.to_bits() == g.value(i * 2 + f).to_bits(),
                        "Staged surface mismatch"
                    );
                }
            }
        }
    }
    let evidence = json!({"rows":nc*nt,"storage_rows":storage_rows,"table":table,"database":database,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"samples":samples.len(),"staged_comparison":staged,"storage":m.get("storage"),"validated_at":now()});
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
    let cells = path.join("cells.npz");
    if cells.exists() {
        fs::remove_file(cells)?;
    }
    atomic(path.join("discarded.json"), &json!({"time":now()}))
}
