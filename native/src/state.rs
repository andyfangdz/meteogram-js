use anyhow::{bail, Context, Result};
use chrono::{DateTime, NaiveDateTime, Utc};
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub const MODELS: [&str; 3] = ["hrrr", "gfs", "aifs"];
pub const DATABASE: &str = "meteogram_native";
pub const RESERVE: u64 = 20 << 30;
static EVENT_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
pub fn now() -> f64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs_f64()
}
pub fn ident(s: &str) -> Result<&str> {
    if s.is_empty()
        || !s.as_bytes()[0].is_ascii_lowercase()
        || !s
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_')
    {
        bail!("Unsafe identifier");
    }
    Ok(s)
}
pub fn timestamp(s: &str) -> Result<f64> {
    if let Ok(v) = DateTime::parse_from_rfc3339(s) {
        return Ok(v.timestamp() as f64);
    }
    Ok(NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f")?
        .and_utc()
        .timestamp() as f64)
}
pub fn read_json(p: impl AsRef<Path>) -> Result<Value> {
    Ok(serde_json::from_slice(
        &fs::read(p.as_ref()).with_context(|| format!("Reading {}", p.as_ref().display()))?,
    )?)
}
pub fn atomic(p: impl AsRef<Path>, v: &impl Serialize) -> Result<()> {
    let p = p.as_ref();
    fs::create_dir_all(p.parent().context("Missing parent")?)?;
    let tmp = p.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
    let mut f = OpenOptions::new().write(true).create_new(true).open(&tmp)?;
    f.write_all(&serde_json::to_vec_pretty(v)?)?;
    f.sync_all()?;
    fs::rename(&tmp, p)?;
    File::open(p.parent().unwrap())?.sync_all()?;
    Ok(())
}
pub struct Lock(File);
impl Lock {
    pub fn acquire(root: &Path, name: &str, exclusive: bool, nonblocking: bool) -> Result<Self> {
        fs::create_dir_all(root)?;
        let f = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(root.join(format!("{name}.lock")))?;
        match (exclusive, nonblocking) {
            (true, true) => f.try_lock_exclusive()?,
            (true, false) => f.lock_exclusive()?,
            (false, true) => FileExt::try_lock_shared(&f)?,
            (false, false) => FileExt::lock_shared(&f)?,
        };
        Ok(Self(f))
    }
}
impl Drop for Lock {
    fn drop(&mut self) {
        let _ = FileExt::unlock(&self.0);
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Entry {
    pub revision: String,
    pub database: String,
    pub table: String,
    #[serde(default)]
    pub owned: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retired_at: Option<f64>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Catalog {
    pub version: u32,
    pub generation: u64,
    pub current: BTreeMap<String, Entry>,
    pub previous: BTreeMap<String, Entry>,
    pub retired: Vec<Entry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub published_at: Option<f64>,
}
impl Default for Catalog {
    fn default() -> Self {
        Self {
            version: 1,
            generation: 0,
            current: BTreeMap::new(),
            previous: BTreeMap::new(),
            retired: vec![],
            published_at: None,
        }
    }
}
pub fn catalog(root: &Path) -> Result<Catalog> {
    let p = root.join("catalog.json");
    if p.exists() {
        let c: Catalog = serde_json::from_slice(&fs::read(p)?)?;
        anyhow::ensure!(c.version == 1, "Unsupported catalog version");
        Ok(c)
    } else {
        Ok(Catalog::default())
    }
}
pub fn descriptor(root: &Path, e: &Entry) -> Result<PathBuf> {
    ident(&e.revision)?;
    ident(&e.database)?;
    ident(&e.table)?;
    Ok(root.join("revisions").join(&e.revision))
}
pub fn leads(model: &str) -> Vec<u16> {
    match model {
        "hrrr" => (0..49).collect(),
        "gfs" => (0..121).chain((123..181).step_by(3)).collect(),
        "aifs" => (0..181).step_by(6).collect(),
        _ => vec![],
    }
}
pub fn variables(model: &str) -> Vec<String> {
    let names = match model {
        "gfs" => vec![
            "cloud_cover",
            "geopotential_height",
            "temperature",
            "wind_u",
            "wind_v",
            "relative_humidity",
        ],
        "hrrr" => vec![
            "geopotential_height",
            "temperature",
            "wind_u",
            "wind_v",
            "relative_humidity",
            "dew_point_temperature",
        ],
        _ => vec![
            "geopotential_height",
            "temperature",
            "wind_u",
            "wind_v",
            "specific_humidity",
        ],
    };
    names.into_iter().map(str::to_owned).collect()
}
pub fn fields(m: &Value) -> Vec<String> {
    m["variables"]
        .as_array()
        .unwrap()
        .iter()
        .chain(m["surface_variables"].as_array().unwrap())
        .map(|s| s.as_str().unwrap().to_owned())
        .collect()
}
pub fn dataset(model: &str) -> Result<&'static str> {
    Ok(match model {
        "hrrr" => "noaa-hrrr-forecast-48-hour-virtual",
        "gfs" => "noaa-gfs-forecast-virtual",
        "aifs" => "ecmwf-aifs-single-forecast-virtual",
        _ => bail!("Unknown model"),
    })
}
pub fn freshness(m: &Value) -> Result<Value> {
    let model = m["model"].as_str().context("Model missing")?;
    let age = (now() - timestamp(m["init_time"].as_str().context("Missing init")?)?) / 3600.;
    let remaining = m["leads"]
        .as_array()
        .context("Missing leads")?
        .last()
        .and_then(Value::as_f64)
        .context("Empty leads")?
        - age;
    let (max_age, min_remaining) = if model == "hrrr" {
        (24., 12.)
    } else {
        (48., 72.)
    };
    let round = |x: f64| (x * 1000.).round() / 1000.;
    Ok(
        json!({"init_time":m["init_time"],"available":(-1. ..=max_age).contains(&age)&&remaining>=min_remaining,"age_hours":round(age),"remaining_hours":round(remaining),"max_age_hours":max_age as u32,"min_remaining_hours":min_remaining as u32}),
    )
}
pub fn check_metadata(m: &Value) -> Result<()> {
    let model = m["model"].as_str().context("Missing model")?;
    dataset(model)?;
    let levels: Vec<u16> = if model == "aifs" {
        vec![1000, 925, 850, 700, 600, 500, 400, 300]
    } else {
        (275..1001).step_by(25).rev().collect()
    };
    if m["dataset"] != dataset(model)?
        || m["coverage"] != "conus"
        || m["spatial_shards"] != 1
        || m["cells"].as_u64() != Some(if model == "hrrr" { 1638757 } else { 24885 })
        || m["leads"] != json!(leads(model))
        || !numeric_array_eq(&m["levels"], &json!(levels))
        || m["variables"] != json!(variables(model))
        || m["surface_variables"] != json!(["temperature_2m", "dew_point_temperature_2m"])
    {
        bail!("Source schema/coverage changed");
    }
    if let Some(s) = m.get("storage") {
        crate::dictionary::index_bytes(m)?;
        if crate::packed::tiled(m) {
            anyhow::ensure!(
                crate::packed::valid_layout(m)
                    && s["encoding"] == crate::dictionary::ENCODING
                    && s["codec"] == "delta"
                    && s["delta_bytes"] == 2,
                "Unknown packed storage layout"
            );
        } else if s["layout"] != "cell_profile_v1"
            || !["level_lead", "lead_level"].iter().any(|v| s["axes"] == *v)
            || !["float32_bits", "float32", crate::dictionary::ENCODING]
                .iter()
                .any(|v| s["encoding"] == *v)
            || !["t64", "delta", "zstd", "lz4", "gorilla"]
                .iter()
                .any(|v| s["codec"] == *v)
            || (["t64", "delta"].iter().any(|v| s["codec"] == *v)
                != (s["encoding"] == "float32_bits"
                    || s["encoding"] == crate::dictionary::ENCODING))
        {
            bail!("Unknown storage layout");
        }
    }
    Ok(())
}
pub fn numeric_array_eq(a: &Value, b: &Value) -> bool {
    match (a.as_array(), b.as_array()) {
        (Some(a), Some(b)) => {
            a.len() == b.len()
                && a.iter()
                    .zip(b)
                    .all(|(x, y)| x.as_f64().is_some() && x.as_f64() == y.as_f64())
        }
        _ => false,
    }
}
pub fn publish(root: &Path, model: &str, e: Entry, rollback: bool) -> Result<Catalog> {
    let path = descriptor(root, &e)?;
    let m = read_json(path.join("metadata.json"))?;
    check_metadata(&m)?;
    let v = read_json(path.join("validated.json"))?;
    if v["table"] != e.table
        || v["database"] != e.database
        || m["model"] != model
        || v["init_time"] != m["init_time"]
        || v["snapshot_id"] != m["snapshot_id"]
        || v.get("storage").unwrap_or(&Value::Null) != m.get("storage").unwrap_or(&Value::Null)
        || freshness(&m)?["available"] != true
    {
        bail!("Candidate lacks matching, fresh validation");
    }
    let _guard = Lock::acquire(root, "catalog", true, false)?;
    let mut c = catalog(root)?;
    if let Some(old) = c.current.get(model) {
        let om = read_json(descriptor(root, old)?.join("metadata.json"))?;
        if !rollback
            && timestamp(m["init_time"].as_str().unwrap())?
                <= timestamp(om["init_time"].as_str().unwrap())?
        {
            bail!("Publication must advance initialization");
        }
    }
    if let Some(mut prev) = c.previous.remove(model) {
        if prev.revision != e.revision {
            prev.retired_at = Some(now());
            c.retired.push(prev);
        }
    }
    if let Some(old) = c.current.insert(model.to_owned(), e) {
        c.previous.insert(model.to_owned(), old);
    }
    c.generation += 1;
    c.published_at = Some(now());
    atomic(root.join("catalog.json"), &c)?;
    Ok(c)
}
pub fn disk_guard(root: &Path, extra: u64) -> Result<u64> {
    let free = fs2::available_space(root)?;
    if free < RESERVE + extra {
        bail!(
            "Insufficient disk: {free} free; require {}",
            RESERVE + extra
        );
    }
    Ok(free)
}
pub fn admission(model: &str) -> u64 {
    match model {
        "hrrr" => 210 << 30,
        "gfs" => 35 << 30,
        _ => 5 << 30,
    }
}
pub fn candidate(root: &Path, model: &str) -> Result<(PathBuf, Entry)> {
    dataset(model)?;
    let revision = format!(
        "{}_{}_{}",
        model,
        Utc::now().format("%Y%m%d%H%M%S"),
        &uuid::Uuid::new_v4().simple().to_string()[..8]
    );
    let e = Entry {
        table: revision.clone(),
        revision,
        database: DATABASE.into(),
        owned: true,
        retired_at: None,
    };
    let p = descriptor(root, &e)?;
    fs::create_dir_all(&p)?;
    atomic(p.join("entry.json"), &e)?;
    Ok((p, e))
}
pub fn event(root: &Path, v: Value) -> Result<()> {
    let _guard = EVENT_LOCK
        .lock()
        .map_err(|_| anyhow::anyhow!("Event log lock poisoned"))?;
    let mut v = v;
    v["time"] = json!(now());
    let line = serde_json::to_string(&v)?;
    println!("{line}");
    let path = root.join("events.jsonl");
    if path.exists() && path.metadata()?.len() > 20 << 20 {
        fs::rename(&path, root.join("events-previous.jsonl"))?;
    }
    let mut f = OpenOptions::new().append(true).create(true).open(path)?;
    writeln!(f, "{line}")?;
    Ok(())
}
pub struct Npy {
    pub dtype: String,
    pub bytes: Vec<u8>,
}
pub fn parse_npy(b: &[u8]) -> Result<(Vec<usize>, String, usize)> {
    if b.len() < 12 || &b[..6] != b"\x93NUMPY" {
        bail!("Invalid NPY");
    }
    let (n, start) = if b[6] == 1 {
        (u16::from_le_bytes(b[8..10].try_into()?) as usize, 10)
    } else if b[6] == 2 {
        (u32::from_le_bytes(b[8..12].try_into()?) as usize, 12)
    } else {
        bail!("Unsupported NPY version")
    };
    let h = std::str::from_utf8(b.get(start..start + n).context("Truncated header")?)?;
    if !h.contains("'fortran_order': False") {
        bail!("Fortran NPY unsupported");
    }
    let dtype = h
        .split("'descr': '")
        .nth(1)
        .context("Missing dtype")?
        .split('\'')
        .next()
        .unwrap()
        .to_owned();
    let dims = h
        .split("'shape': (")
        .nth(1)
        .context("Missing shape")?
        .split(')')
        .next()
        .unwrap();
    let shape = dims
        .split(',')
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().parse())
        .collect::<std::result::Result<Vec<usize>, _>>()?;
    Ok((shape, dtype, start + n))
}
#[cfg(test)]
pub fn npy_bytes(dtype: &str, shape: &[usize], bytes: &[u8]) -> Vec<u8> {
    let dims = shape
        .iter()
        .map(usize::to_string)
        .collect::<Vec<_>>()
        .join(", ");
    let mut h = format!("{{'descr': '{dtype}', 'fortran_order': False, 'shape': ({dims},), }}");
    while (10 + h.len() + 1) % 64 != 0 {
        h.push(' ');
    }
    h.push('\n');
    let mut b = b"\x93NUMPY\x01\x00".to_vec();
    b.extend_from_slice(&(h.len() as u16).to_le_bytes());
    b.extend_from_slice(h.as_bytes());
    b.extend_from_slice(bytes);
    b
}
pub fn load_npz(p: &Path) -> Result<BTreeMap<String, Npy>> {
    let mut z = zip::ZipArchive::new(File::open(p)?)?;
    let mut out = BTreeMap::new();
    for i in 0..z.len() {
        let mut f = z.by_index(i)?;
        let mut b = vec![];
        f.read_to_end(&mut b)?;
        let (_shape, dtype, start) = parse_npy(&b)?;
        out.insert(
            f.name().trim_end_matches(".npy").to_owned(),
            Npy {
                dtype,
                bytes: b[start..].to_vec(),
            },
        );
    }
    Ok(out)
}
#[cfg(test)]
pub fn save_cells(p: &Path, ids: &[u32], lat: &[f64], lon: &[f64]) -> Result<()> {
    let mut z = zip::ZipWriter::new(File::create(p)?);
    let opts =
        zip::write::SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
    let idb: Vec<u8> = ids.iter().flat_map(|v| v.to_le_bytes()).collect();
    for (name, dtype, b) in [
        ("cell_id", "<u4", idb),
        (
            "latitude",
            "<f8",
            lat.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
        (
            "longitude",
            "<f8",
            lon.iter().flat_map(|v| v.to_le_bytes()).collect(),
        ),
    ] {
        z.start_file(format!("{name}.npy"), opts)?;
        z.write_all(&npy_bytes(dtype, &[ids.len()], &b))?;
    }
    z.finish()?.sync_all()?;
    Ok(())
}
pub fn clean_workspace(p: &Path) -> Result<()> {
    for item in fs::read_dir(p)? {
        let item = item?;
        let s = item.file_name().to_string_lossy().into_owned();
        if s.starts_with("lead-") && s.ends_with(".palette") && item.file_type()?.is_dir() {
            fs::remove_dir_all(item.path())?;
            continue;
        }
        if (s.starts_with("lead-") || s.starts_with("surface-"))
            && [".npy", ".stage.zst", ".stage.json", ".stage.zst.tmp"]
                .iter()
                .any(|suffix| s.ends_with(suffix))
        {
            fs::remove_file(item.path())?;
        }
    }
    let c = p.join("source-cache");
    if c.exists() {
        fs::remove_dir_all(c)?;
    }
    Ok(())
}
