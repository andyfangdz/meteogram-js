use crate::{grid::Lambert, state::*};
use anyhow::{bail, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use icechunk::{
    config::{Credentials, GcsCredentials, ObjectStoreConfig, S3Credentials},
    format::ByteRange,
    repository::VersionInfo,
    storage::new_http_storage,
    store::Store,
    Repository,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};
use tokio::sync::RwLock;
/// Expected upstream incompleteness, distinct from transport/schema/storage errors.
#[derive(Debug)]
pub struct SourcePending(pub &'static str);
impl std::fmt::Display for SourcePending {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} data not yet available", self.0)
    }
}
impl std::error::Error for SourcePending {}

#[derive(Clone)]
pub struct Source {
    pub store: Arc<SourceStore>,
    pub snapshot: String,
    pub collection: Value,
    // A Source always owns one immutable readonly snapshot. Keep only its
    // small metadata/coordinate arrays here, never weather payloads.
    metadata: Arc<RwLock<HashMap<String, Value>>>,
    coordinates: Arc<RwLock<HashMap<String, Vec<f64>>>>,
    map_slots: Arc<tokio::sync::Semaphore>,
}
pub enum SourceStore {
    Icechunk(Arc<Store>),
    Ecmwf(Arc<crate::ecmwf::DirectSource>),
}
impl SourceStore {
    async fn get(&self, key: &str, range: &ByteRange) -> Result<Vec<u8>> {
        match self {
            Self::Icechunk(store) => Ok(store.get(key, range).await?.to_vec()),
            Self::Ecmwf(source) => source.get(key).await,
        }
    }
    async fn exists(&self, key: &str) -> Result<bool> {
        match self {
            Self::Icechunk(store) => Ok(store.exists(key).await?),
            Self::Ecmwf(source) => Ok(source.exists(key)),
        }
    }
    fn icechunk(&self) -> Result<&Store> {
        match self {
            Self::Icechunk(store) => Ok(store),
            _ => bail!("This diagnostic requires an Icechunk source"),
        }
    }
    fn validate_cached(&self, key: &str, bytes: &[u8]) -> Result<()> {
        if let Self::Ecmwf(source) = self {
            source.validate(key, bytes)?;
        }
        Ok(())
    }
}
static REPOSITORIES: std::sync::OnceLock<tokio::sync::Mutex<HashMap<String, Arc<Repository>>>> =
    std::sync::OnceLock::new();
impl Source {
    pub fn from_ecmwf(direct: crate::ecmwf::DirectSource) -> Result<Self> {
        let snapshot = if direct.hours.len() == 1 {
            crate::ecmwf::DirectSource::hour_snapshot(&direct.hours[0])?
        } else {
            direct.snapshot()?
        };
        let product = dataset(&direct.model)?;
        let model = direct.model.clone();
        Ok(Self {
            store: Arc::new(SourceStore::Ecmwf(Arc::new(direct))),
            snapshot,
            collection: json!({"id":product,"source_provider":crate::ecmwf::provider(&model)}),
            metadata: Arc::new(RwLock::new(HashMap::new())),
            coordinates: Arc::new(RwLock::new(HashMap::new())),
            map_slots: Arc::new(tokio::sync::Semaphore::new(16)),
        })
    }
    pub fn hour_pin(&self, pin: &Value, lead: u16) -> Result<Value> {
        match &*self.store {
            SourceStore::Ecmwf(direct) => direct.pin(pin, lead),
            _ => Ok(pin.clone()),
        }
    }
    pub async fn open(model: &str, pin: Option<&Value>) -> Result<Self> {
        if let Some(pin) = pin.filter(|p| crate::ecmwf::is_direct(&p["source_provider"])) {
            anyhow::ensure!(
                matches!(model, "aifs" | "ifs" | "aigfs"),
                "Invalid direct source model"
            );
            return Self::from_ecmwf(crate::ecmwf::DirectSource::from_pin(pin)?);
        }
        if matches!(model, "ifs" | "aigfs") {
            anyhow::ensure!(
                pin.is_none_or(|p| p.get("snapshot_id").is_none()),
                "Direct model requires its pinned source descriptor"
            );
            return Self::from_ecmwf(
                crate::ecmwf::discover_model(model, direct_cycles(now() as i64)).await?,
            );
        }
        let product = pin
            .and_then(|p| p["dataset"].as_str())
            .unwrap_or(dataset(model)?);
        anyhow::ensure!(valid_dataset(model, product), "Unsupported source product");
        let collection: Value = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?
            .get(format!(
                "https://stac.dynamical.org/{}/collection.json",
                product
            ))
            .send()
            .await?
            .error_for_status()?
            .json()
            .await?;
        let a = &collection["assets"]["icechunk-https"];
        let href = a["href"].as_str().context("Missing Icechunk URL")?;
        let mut creds = HashMap::new();
        for c in a["icechunk:virtual_chunk_containers"]
            .as_array()
            .context("Missing containers")?
        {
            anyhow::ensure!(
                c["credentials"]["anonymous"] == true,
                "Only public anonymous source containers are supported"
            );
            let credential = match c["credentials"]["type"].as_str() {
                Some("s3") => Credentials::S3(S3Credentials::Anonymous),
                Some("gcs") => Credentials::Gcs(GcsCredentials::Anonymous),
                _ => bail!("Unknown virtual storage"),
            };
            creds.insert(
                c["url_prefix"]
                    .as_str()
                    .context("Missing URL prefix")?
                    .to_owned(),
                Some(credential),
            );
        }
        let cache = REPOSITORIES.get_or_init(|| tokio::sync::Mutex::new(HashMap::new()));
        let cache_key = format!("{product}:{href}");
        let cached = cache.lock().await.get(&cache_key).cloned();
        let repo = if let Some(repo) = cached {
            repo
        } else {
            let storage = new_http_storage(href, None, None)?;
            let repo = Repository::open(storage)
                .config(initial_config(model))
                .authorize_virtual_chunk_access(creds.clone())
                .execute()
                .await?;
            // Icechunk 2.3's object_store S3 backend applies S3Options after
            // credentials, so its persisted anonymous=false overrides Anonymous.
            // Preserve explicit public access for the STAC-authorized containers;
            // never fall back to host AWS credentials or EC2 metadata.
            let mut config = repo.config().clone();
            public_config(&mut config, &creds);
            let repo = Arc::new(repo.reopen(Some(config), None).await?);
            let mut cache = cache.lock().await;
            cache.retain(|key, _| !key.starts_with(&format!("{product}:")));
            cache.insert(cache_key, repo.clone());
            repo
        };
        let version = if let Some(snapshot) = pin.and_then(|p| p["snapshot_id"].as_str()) {
            VersionInfo::SnapshotId(snapshot.try_into().map_err(anyhow::Error::msg)?)
        } else {
            VersionInfo::BranchTipRef("main".into())
        };
        let session = repo.readonly_session(&version).await?;
        let snapshot = session.snapshot_id().to_string();
        let store = Arc::new(Store::from_session(Arc::new(RwLock::new(session))).await);
        Ok(Self {
            store: Arc::new(SourceStore::Icechunk(store)),
            snapshot,
            collection,
            metadata: Arc::new(RwLock::new(HashMap::new())),
            coordinates: Arc::new(RwLock::new(HashMap::new())),
            map_slots: Arc::new(tokio::sync::Semaphore::new(16)),
        })
    }
    pub async fn meta(&self, name: &str) -> Result<Value> {
        if let SourceStore::Ecmwf(source) = &*self.store {
            return source.meta(name);
        }
        if let Some(value) = self.metadata.read().await.get(name) {
            return Ok(value.clone());
        }
        let bytes = self
            .store
            .get(&format!("{name}/zarr.json"), &ByteRange::ALL)
            .await?;
        let value: Value = serde_json::from_slice(&bytes)?;
        self.metadata
            .write()
            .await
            .insert(name.to_owned(), value.clone());
        Ok(value)
    }
    pub async fn coord(&self, name: &str) -> Result<Vec<f64>> {
        if let SourceStore::Ecmwf(source) = &*self.store {
            return source.coord(name);
        }
        if let Some(values) = self.coordinates.read().await.get(name) {
            return Ok(values.clone());
        }
        let m = self.meta(name).await?;
        let shape = m["shape"].as_array().context("No shape")?;
        anyhow::ensure!(shape.len() == 1, "Expected 1D coordinate");
        let len = shape[0].as_u64().context("Bad coordinate length")? as usize;
        let chunk = m["chunk_grid"]["configuration"]["chunk_shape"][0]
            .as_u64()
            .context("Bad chunk length")? as usize;
        let mut values = vec![];
        for i in 0..len.div_ceil(chunk) {
            let bytes = self
                .store
                .get(&format!("{name}/c/{i}"), &ByteRange::ALL)
                .await?;
            let mut data = decode(&m, &bytes)?;
            values.append(&mut data);
        }
        values.truncate(len);
        self.coordinates
            .write()
            .await
            .insert(name.to_owned(), values.clone());
        Ok(values)
    }
    pub async fn init_times(&self) -> Result<Vec<f64>> {
        let values = self.coord("init_time").await?;
        let m = self.meta("init_time").await?;
        let units = m["attributes"]["units"]
            .as_str()
            .context("Missing init units")?;
        let (unit, epoch) = units.split_once(" since ").context("Invalid init units")?;
        let factor = match unit {
            "seconds" => 1.,
            "hours" => 3600.,
            "nanoseconds" => 1e-9,
            _ => bail!("Unsupported init time units"),
        };
        let epoch = epoch.replace(' ', "T");
        let epoch = if epoch.len() == 10 {
            format!("{epoch}T00:00:00")
        } else {
            epoch
        };
        let epoch = timestamp(&epoch)?;
        Ok(values.into_iter().map(|v| epoch + v * factor).collect())
    }
}
pub(crate) fn initial_config(model: &str) -> icechunk::RepositoryConfig {
    let mut config = icechunk::RepositoryConfig::default();
    if matches!(model, "hrrr" | "rrfs") {
        // Regional pressure/cloud arrays exceed the default reference cache.
        // Keep bounded manifest references, without weather payloads. Set this
        // at initial open:
        // Icechunk 2.3's reopen shares the original asset manager and caches.
        config.caching = Some(icechunk::config::CachingConfig {
            num_chunk_refs: Some(8_000_000),
            num_bytes_chunks: Some(0),
            ..Default::default()
        });
    }
    config
}
pub(crate) fn public_config(
    config: &mut icechunk::RepositoryConfig,
    creds: &HashMap<String, Option<Credentials>>,
) {
    if let Some(containers) = config.virtual_chunk_containers.as_mut() {
        for container in containers.values_mut() {
            if matches!(
                creds.get(container.url_prefix()),
                Some(Some(Credentials::S3(S3Credentials::Anonymous)))
            ) {
                match &mut container.store {
                    ObjectStoreConfig::S3(opts) | ObjectStoreConfig::S3Compatible(opts) => {
                        opts.anonymous = true
                    }
                    _ => {}
                }
            }
        }
    }
}
pub(crate) fn decode(m: &Value, b: &[u8]) -> Result<Vec<f64>> {
    let codecs = m["codecs"].as_array().context("No codecs")?;
    let mut bytes = b.to_vec();
    let mut data: Option<Vec<f64>> = None;
    for codec in codecs.iter().rev() {
        let name = codec["name"].as_str().context("No codec name")?;
        match name {
            "blosc" => {
                anyhow::ensure!(bytes.len() >= 16, "Truncated Blosc header");
                let size = u32::from_le_bytes(bytes[4..8].try_into()?) as usize;
                anyhow::ensure!(size <= 32 * 1024 * 1024, "Oversized coordinate chunk");
                bytes =
                    blusc::convenience::blosc1_decompress(&bytes).map_err(anyhow::Error::msg)?;
                anyhow::ensure!(bytes.len() == size, "Blosc size mismatch");
            }
            "scale_offset" => {
                let c = &codec["configuration"];
                let parse = |v: &Value, default: f64| -> Result<f64> {
                    if v.is_null() {
                        Ok(default)
                    } else if let Some(n) = v.as_f64() {
                        Ok(n)
                    } else {
                        Ok(v.as_str().context("Invalid scale/offset")?.parse()?)
                    }
                };
                let scale = parse(&c["scale"], 1.)?;
                let offset = parse(&c["offset"], 0.)?;
                anyhow::ensure!(scale != 0., "Zero scale");
                for x in data.as_mut().context("Scale/offset before array decode")? {
                    if scale != 1. {
                        *x /= scale;
                    }
                    if offset != 0. {
                        *x += offset;
                    }
                }
            }
            "zstd" => bytes = zstd::decode_all(bytes.as_slice())?,
            "bytes" => {
                let endian = codec["configuration"]["endian"]
                    .as_str()
                    .unwrap_or("little");
                anyhow::ensure!(endian == "little", "Unsupported byte order");
                let dtype = m["data_type"].as_str().context("No dtype")?;
                let mut v = vec![];
                match dtype {
                    "float64" => {
                        anyhow::ensure!(bytes.len().is_multiple_of(8), "Truncated f64");
                        for b in bytes.as_chunks::<8>().0.iter() {
                            v.push(f64::from_le_bytes(*b));
                        }
                    }
                    "float32" => {
                        for b in bytes.as_chunks::<4>().0.iter() {
                            v.push(f32::from_le_bytes(*b) as f64);
                        }
                    }
                    "int64" => {
                        for b in bytes.as_chunks::<8>().0.iter() {
                            v.push(i64::from_le_bytes(*b) as f64);
                        }
                    }
                    "int32" => {
                        for b in bytes.as_chunks::<4>().0.iter() {
                            v.push(i32::from_le_bytes(*b) as f64);
                        }
                    }
                    _ => bail!("Unsupported dtype {dtype}"),
                };
                data = Some(v);
            }
            "gribberish" => {
                let msg =
                    gribberish::message::Message::from_data(&bytes, 0).context("Invalid GRIB")?;
                let conf = &codec["configuration"];
                let raw = msg.data()?;
                let proj = msg.latlng_projector()?;
                data = Some(proj.adjust_data(
                    raw,
                    conf["adjust_longitude_range"].as_bool().unwrap_or(false),
                    conf["north_up"].as_bool().unwrap_or(false),
                ));
            }
            _ => bail!("Unsupported Zarr codec {name}"),
        }
    }
    let mut v = data.context("No array codec")?;
    let attrs = &m["attributes"];
    let scale = attrs["scale_factor"].as_f64().unwrap_or(1.);
    let offset = attrs["add_offset"].as_f64().unwrap_or(0.);
    let fill = attrs["_FillValue"].as_f64();
    for x in &mut v {
        if fill == Some(*x) {
            *x = f64::NAN;
        } else {
            if scale != 1. {
                *x *= scale;
            }
            if offset != 0. {
                *x += offset;
            }
        }
    }
    Ok(v)
}
#[derive(Clone)]
struct Task {
    field: usize,
    level: Option<usize>,
    name: String,
    meta: Value,
    key: String,
}
pub(crate) fn cache_path(dir: &Path, key: &str) -> PathBuf {
    let digest = format!("{:x}", Sha256::digest(key));
    dir.join(&digest[..2]).join(&digest)
}
pub(crate) fn prune_checkpointed_cache(dir: &Path, keys: &[String]) -> Result<u64> {
    let mut removed = 0;
    for key in keys {
        let path = cache_path(dir, key);
        match fs::metadata(&path) {
            Ok(metadata) => {
                fs::remove_file(&path)?;
                let checksum = path.with_extension("sha256");
                if checksum.exists() {
                    fs::remove_file(checksum)?;
                }
                removed += metadata.len();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(removed)
}
fn cloud_cache_dir(out: &Path, snapshot: &str) -> Result<PathBuf> {
    let root = out.join("source-cache");
    fs::create_dir_all(&root)?;
    let namespace = format!("cloud-{:x}", Sha256::digest(snapshot));
    for entry in fs::read_dir(&root)? {
        let entry = entry?;
        let name = entry.file_name();
        if name.to_string_lossy().starts_with("cloud-")
            && name != namespace.as_str()
            && entry.file_type()?.is_dir()
        {
            fs::remove_dir_all(entry.path())?;
        }
    }
    let path = root.join(namespace);
    fs::create_dir_all(&path)?;
    Ok(path)
}
async fn cached(store: &SourceStore, dir: &Path, key: &str, bytes: &AtomicU64) -> Result<Vec<u8>> {
    let p = cache_path(dir, key);
    let direct = matches!(store, SourceStore::Ecmwf(_));
    let checksum = p.with_extension("sha256");
    if p.exists() && (!direct || checksum.exists()) {
        let bytes = fs::read(p)?;
        if direct {
            anyhow::ensure!(
                read_json(&checksum)? == json!(crate::ecmwf::digest(&bytes)),
                "Cached ECMWF message checksum differs"
            );
        }
        store.validate_cached(key, &bytes)?;
        return Ok(bytes);
    }
    let mut last = None;
    for attempt in 0..4 {
        match store.get(key, &ByteRange::ALL).await {
            Ok(b) => {
                disk_guard(dir, 0)?;
                fs::create_dir_all(p.parent().unwrap())?;
                let tmp = p.with_extension(format!("{}.tmp", uuid::Uuid::new_v4().simple()));
                fs::write(&tmp, &b)?;
                fs::rename(tmp, &p)?;
                if direct {
                    atomic(&checksum, &json!(crate::ecmwf::digest(&b)))?;
                }
                bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
                return Ok(b);
            }
            Err(e) => {
                if e.downcast_ref::<crate::noaa::RateLimited>().is_some() {
                    return Err(e);
                }
                last = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
            }
        }
    }
    bail!("Missing source chunk {key}: {:?}", last)
}
#[cfg(test)]
pub async fn fetch_pressure_hour(
    model: &str,
    out: &Path,
    pin: Value,
    lead: u16,
    available: &Source,
) -> Result<Value> {
    fetch_hour_inner(model, out, pin, lead, available, false).await
}

pub async fn fetch_hour(
    model: &str,
    out: &Path,
    pin: Value,
    lead: u16,
    available: &Source,
) -> Result<Value> {
    fetch_hour_inner(model, out, pin, lead, available, model == "hrrr").await
}

async fn fetch_hour_inner(
    model: &str,
    out: &Path,
    pin: Value,
    lead: u16,
    available: &Source,
    with_clouds: bool,
) -> Result<Value> {
    fs::create_dir_all(out)?;
    let _lock = Lock::acquire(out, "fetch", true, true)?;
    let start = Instant::now();
    let existing = out.join("metadata.json").exists();
    let old = if existing {
        read_json(out.join("metadata.json"))?
    } else if pin.get("ecmwf_source").is_some() {
        pin
    } else {
        available.hour_pin(&pin, lead)?
    };
    anyhow::ensure!(
        old["model"] == model
            && old["dataset"]
                .as_str()
                .is_some_and(|d| valid_dataset(model, d)),
        "Pinned metadata belongs to another model"
    );
    anyhow::ensure!(
        !existing || crate::dictionary::staged(&old),
        "Resume requires dictionary staging"
    );
    let opened;
    let src = if existing || crate::ecmwf::is_direct(&old["source_provider"]) {
        let mut source = Source::open(model, Some(&old)).await?;
        source.map_slots = available.map_slots.clone();
        opened = source;
        &opened
    } else {
        available
    };
    let init_seconds = src.init_times().await?;
    let pinned_init = timestamp(
        old["init_time"]
            .as_str()
            .context("Missing pinned initialization")?,
    )?;
    let init_index = init_seconds
        .iter()
        .position(|v| (*v - pinned_init).abs() < 0.001)
        .context("Pinned initialization unavailable")?;
    let init_time = chrono::DateTime::from_timestamp(init_seconds[init_index] as i64, 0)
        .context("Bad init")?
        .format("%Y-%m-%dT%H:%M:%S%.9f")
        .to_string();
    let source_leads = src.coord("lead_time").await?;
    let lead_meta = src.meta("lead_time").await?;
    let lead_units = lead_meta["attributes"]["units"]
        .as_str()
        .context("Missing lead units")?;
    let lead_factor = match lead_units {
        "seconds" => 1. / 3600.,
        "hours" => 1.,
        "nanoseconds" => 1. / 3.6e12,
        _ => bail!("Unsupported lead units {lead_units}"),
    };
    let source_leads: Vec<u16> = source_leads
        .iter()
        .map(|v| (*v * lead_factor).round() as u16)
        .collect();
    let pressure = src.coord("pressure_level/pressure_level").await?;
    let mut levels: Vec<u16> = pressure
        .iter()
        .copied()
        .filter(|v| *v >= 275. && *v <= 1000.)
        .map(|v| v as u16)
        .collect();
    levels.sort_unstable_by(|a, b| b.cmp(a));
    let mut ids = vec![];
    let mut lat = vec![];
    let mut lon = vec![];
    let grid = if grid_relative(model) {
        let xs = src.coord("x").await?;
        let ys = src.coord("y").await?;
        let cf = src.meta("spatial_ref").await?;
        let proj = Lambert::from_cf(&cf["attributes"])?;
        for (y, py) in ys.iter().enumerate() {
            for (x, px) in xs.iter().enumerate() {
                let (la, lo) = proj.inverse(*px, *py);
                if (24. ..=50.).contains(&la) && (-125. ..=-66.).contains(&lo) {
                    ids.push((y * xs.len() + x) as u32);
                    lat.push(la);
                    lon.push(lo);
                }
            }
        }
        json!({"kind":"projected","crs":proj.crs(),"x":xs,"y":ys,"tiles":[],"native_cells":xs.len()*ys.len(),"conus_cells":ids.len()})
    } else {
        let ys = src.coord("latitude").await?;
        let xs = src.coord("longitude").await?;
        for (y, la) in ys.iter().enumerate() {
            for (x, lo) in xs.iter().enumerate() {
                let lo = (*lo + 180.).rem_euclid(360.) - 180.;
                if (24. ..=50.).contains(la) && (-125. ..=-66.).contains(&lo) {
                    ids.push((y * xs.len() + x) as u32);
                    lat.push(*la);
                    lon.push(lo);
                }
            }
        }
        json!({"kind":"latlon","latitude":ys,"longitude":xs,"native_cells":ys.len()*xs.len(),"conus_cells":ids.len()})
    };
    let variables = variables(model);
    let surface = surface_variables(model);
    let mut metas = HashMap::new();
    let mut units = serde_json::Map::new();
    for v in &variables {
        let name = format!("pressure_level/{v}");
        let m = src.meta(&name).await?;
        units.insert(v.clone(), m["attributes"]["units"].clone());
        metas.insert(name, m);
    }
    for &v in &surface {
        metas.insert(v.to_owned(), src.meta(v).await?);
    }
    let product = src.collection["id"]
        .as_str()
        .context("Missing collection id")?;
    anyhow::ensure!(valid_dataset(model, product), "Collection identity differs");
    anyhow::ensure!(
        product_leads(model, product).contains(&lead),
        "Invalid forecast hour"
    );
    let leads = vec![lead];
    let mut meta = json!({"model":model,"dataset":product,"init_time":init_time,"snapshot_id":src.snapshot,"cells":ids.len(),"levels":levels,"leads":leads,"variables":variables,"surface_variables":surface,"grid":grid,"units":units,"coverage":"conus","spatial_shards":1,"shard_index":0,"map_concurrency":16,"open_seconds":start.elapsed().as_secs_f64()});
    if existing {
        meta["open_seconds"] = old["open_seconds"].clone();
    }
    if crate::ecmwf::is_direct(&old["source_provider"]) {
        meta["source_provider"] = old["source_provider"].clone();
        meta["ecmwf_source"] = old["ecmwf_source"].clone();
        if let Some(proofs) = old.get("source_payload_sha256") {
            meta["source_payload_sha256"] = proofs.clone();
        }
    }
    meta["staging"] = json!({"encoding":crate::dictionary::ENCODING,"complete":false});
    {
        for k in ["init_time", "snapshot_id", "cells", "levels"] {
            if old.get(k).is_none() {
                continue;
            }
            anyhow::ensure!(
                if k == "levels" {
                    numeric_array_eq(&old[k], &meta[k])
                } else {
                    old[k] == meta[k]
                },
                "Extraction identity changed: {k}"
            );
        }
        if existing {
            anyhow::ensure!(
                old["leads"] == meta["leads"],
                "Extraction lead selection changed"
            );
        }
    }
    atomic(out.join("collection.json"), &src.collection)?;
    atomic(out.join("source-arrays.json"), &metas)?;
    atomic(out.join("metadata.json"), &meta)?;
    crate::stage::save_cells(out, &ids, &lat, &lon)?;
    // Both tasks share a 16-map limit. A separate task lets cloud I/O progress
    // while pressure arrays are staged. Joining drains both on failure.
    let cloud_source = if available.collection["id"] == meta["dataset"] {
        available
    } else {
        src
    };
    let cloud_task = if with_clouds {
        let source = cloud_source.clone();
        let path = out.to_owned();
        Some(tokio::spawn(async move {
            fetch_cloud_coverage(&path, &source).await
        }))
    } else {
        None
    };
    let (weather, clouds) = tokio::join!(
        async {
            // Resolve references in the pinned Icechunk manifests before downloading
            // any whole-map payload. Incomplete recent runs otherwise fail minutes in.
            let mut by_lead = vec![];
            for lead in &leads {
                let ti = source_leads
                    .iter()
                    .position(|v| v == lead)
                    .context("Lead unavailable")?;
                let mut tasks = vec![];
                for (field, v) in variables.iter().enumerate() {
                    for (level, p) in levels.iter().enumerate() {
                        let pi = pressure
                            .iter()
                            .position(|x| *x == *p as f64)
                            .context("Pressure unavailable")?;
                        tasks.push(task(
                            field,
                            Some(level),
                            format!("pressure_level/{v}"),
                            &metas,
                            init_index,
                            ti,
                            Some(pi),
                        )?);
                    }
                }
                for (field, v) in surface.iter().enumerate() {
                    tasks.push(task(
                        field,
                        None,
                        (*v).into(),
                        &metas,
                        init_index,
                        ti,
                        None,
                    )?);
                }
                by_lead.push((*lead, tasks));
            }
            let keys = by_lead
                .iter()
                .flat_map(|(_, tasks)| tasks.iter().map(|t| t.key.clone()))
                .collect();
            let references = check_references(keys, |key| {
                let store = src.store.clone();
                async move { store.exists(&key).await }
            })
            .await?;
            atomic(out.join("preflight.json"), &references)?;
            if references["complete"] != true {
                return Err(SourcePending("Source").into());
            }
            let cache = out.join("source-cache");
            fs::create_dir_all(&cache)?;
            let ids = Arc::new(ids);
            let bytes = Arc::new(AtomicU64::new(0));
            let mut palettes: Vec<Vec<Vec<u32>>> = crate::dictionary::widths(&meta)?
                .into_iter()
                .map(|width| (0..width).map(|_| Vec::new()).collect())
                .collect();
            let nt = leads.len();
            let nf = variables.len();
            let mut palette_entries = 0;
            fn merge_palettes(
                full: &mut [Vec<Vec<u32>>],
                partial: crate::dictionary::Dictionary,
                t: usize,
                nt: usize,
                nf: usize,
                entries: &mut usize,
            ) -> Result<()> {
                for (f, field) in partial.fields.into_iter().enumerate() {
                    for (l, palette) in field.into_iter().enumerate() {
                        crate::dictionary::count_entries(entries, palette.len())?;
                        full[f][if f < nf { l * nt + t } else { t }] = palette;
                    }
                }
                Ok(())
            }
            for (time_index, (lead, tasks)) in by_lead.into_iter().enumerate() {
                let completed_keys: Vec<_> = tasks.iter().map(|t| t.key.clone()).collect();
                let report = out.join(format!("lead-{lead:03}.json"));
                if report.exists() {
                    anyhow::ensure!(
                        crate::stage::exists(&out.join(format!("lead-{lead:03}")))
                            && crate::stage::exists(&out.join(format!("surface-{lead:03}"))),
                        "Completed arrays removed; use fresh extraction"
                    );
                    {
                        let checkpoint = out.join(format!("lead-{lead:03}.palette"));
                        let partial_meta = read_json(checkpoint.join("metadata.json"))?;
                        anyhow::ensure!(
                            partial_meta["snapshot_id"] == meta["snapshot_id"]
                                && partial_meta["init_time"] == meta["init_time"]
                                && partial_meta["leads"] == json!([lead]),
                            "Palette checkpoint identity"
                        );
                        let partial =
                            crate::dictionary::Dictionary::load(&checkpoint, &partial_meta)?;
                        merge_palettes(
                            &mut palettes,
                            partial,
                            time_index,
                            nt,
                            nf,
                            &mut palette_entries,
                        )?;
                    }
                    prune_checkpointed_cache(&cache, &completed_keys)?;
                    continue;
                }
                let before = bytes.load(Ordering::Relaxed);
                let started = Instant::now();
                let results = stream::iter(tasks)
                    .map(|t| {
                        let store = src.store.clone();
                        let dir = cache.clone();
                        let ids = ids.clone();
                        let count = bytes.clone();
                        let slots = src.map_slots.clone();
                        async move {
                            let _permit = slots.acquire().await?;
                            let b = cached(&store, &dir, &t.key, &count).await?;
                            tokio::task::spawn_blocking(move || -> Result<_> {
                                let data = decode(&t.meta, &b)?;
                                let mut values = Vec::with_capacity(ids.len());
                                for id in ids.iter() {
                                    let v = *data
                                        .get(*id as usize)
                                        .context("Spatial GRIB shape mismatch")?;
                                    let f = v as f32;
                                    anyhow::ensure!(
                                        v.is_finite() && f.is_finite(),
                                        "Missing/non-finite source {}",
                                        t.name
                                    );
                                    values.push(f);
                                }
                                drop(data);
                                let (values, palette) = crate::dictionary::encode_spatial(values)?;
                                Ok((t, values, palette))
                            })
                            .await?
                        }
                    })
                    .buffer_unordered(16)
                    .collect::<Vec<_>>()
                    .await
                    .into_iter()
                    .collect::<Result<Vec<_>>>()?;
                if let SourceStore::Ecmwf(direct) = &*src.store {
                    let proofs = direct.proofs();
                    anyhow::ensure!(
                        proofs.as_object().is_some_and(
                            |p| p.len() == variables.len() * levels.len() + surface.len()
                        ),
                        "Missing ECMWF payload proofs"
                    );
                    meta["source_payload_sha256"] = proofs;
                    atomic(out.join("metadata.json"), &meta)?;
                }
                let nc = ids.len();
                let nl = levels.len();
                let nf = variables.len();
                let mut profiles = vec![0f32; nc * nl * nf];
                let mut ground = vec![0f32; nc * surface.len()];
                let mut partial_fields: Vec<Vec<Vec<u32>>> = (0..nf + surface.len())
                    .map(|f| {
                        (0..if f < nf { nl } else { 1 })
                            .map(|_| Vec::new())
                            .collect()
                    })
                    .collect();
                for (t, values, palette) in results {
                    {
                        partial_fields[if t.level.is_some() {
                            t.field
                        } else {
                            nf + t.field
                        }][t.level.unwrap_or(0)] = palette;
                    }
                    for (i, v) in values.into_iter().enumerate() {
                        if let Some(l) = t.level {
                            profiles[(i * nl + l) * nf + t.field] = v;
                        } else {
                            ground[i * surface.len() + t.field] = v;
                        }
                    }
                }
                disk_guard(out, ((profiles.len() + ground.len()) * 4) as u64)?;
                let write = crate::stage::write_codes;
                let profile_stage = write(
                    &out.join(format!("lead-{lead:03}")),
                    &[nc, nl, nf],
                    &profiles,
                )?;
                let surface_stage = write(
                    &out.join(format!("surface-{lead:03}")),
                    &[nc, surface.len()],
                    &ground,
                )?;
                {
                    let checkpoint = out.join(format!("lead-{lead:03}.palette"));
                    fs::create_dir_all(&checkpoint)?;
                    let mut partial_meta = meta.clone();
                    partial_meta["leads"] = json!([lead]);
                    let partial = crate::dictionary::Dictionary::new(partial_fields);
                    let info = partial.save(&checkpoint, &partial_meta)?;
                    partial_meta["staging"] = json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
                    atomic(checkpoint.join("metadata.json"), &partial_meta)?;
                    merge_palettes(
                        &mut palettes,
                        partial,
                        time_index,
                        nt,
                        nf,
                        &mut palette_entries,
                    )?;
                }
                File::open(out)?.sync_all()?;
                let stats = json!({"lead":lead,"seconds":started.elapsed().as_secs_f64(),"grib_bytes":bytes.load(Ordering::Relaxed)-before,"chunks":nl*nf+surface.len(),"staged_value_bytes":(profiles.len()+ground.len())*4,"finite":true,"profile_stage":profile_stage,"surface_stage":surface_stage,"source_cache_policy":"uncheckpointed_leads_only"});
                atomic(&report, &stats)?;
                // The synced staged arrays and durable checkpoint now replace these
                // encoded maps for resume. Retain only a partially acquired lead.
                prune_checkpointed_cache(&cache, &completed_keys)?;
                println!("{stats}");
            }
            {
                let dictionary = crate::dictionary::Dictionary::new(palettes);
                let info = dictionary.save(out, &meta)?;
                meta["staging"] = json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
                atomic(out.join("metadata.json"), &meta)?;
            }
            Ok::<_, anyhow::Error>(())
        },
        async {
            if let Some(task) = cloud_task {
                Ok(Some(task.await??))
            } else {
                Ok::<_, anyhow::Error>(None)
            }
        }
    );
    weather?;
    if let Some(clouds) = clouds? {
        meta["native_clouds"] = clouds;
        atomic(out.join("metadata.json"), &meta)?;
    }
    Ok(meta)
}
pub(crate) async fn check_references<F, Fut>(keys: Vec<String>, exists: F) -> Result<Value>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<bool>>,
{
    let started = Instant::now();
    let count = keys.len();
    let mut missing = stream::iter(keys)
        .map(|key| {
            let checked = exists(key.clone());
            async move { Ok::<_, anyhow::Error>(if checked.await? { None } else { Some(key) }) }
        })
        .buffer_unordered(16)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    missing.sort();
    Ok(
        json!({"chunks":count,"missing_count":missing.len(),"missing_examples":missing.iter().take(8).collect::<Vec<_>>(),"complete":missing.is_empty(),"seconds":started.elapsed().as_secs_f64(),"payload_reads":0}),
    )
}
fn task(
    field: usize,
    level: Option<usize>,
    name: String,
    metas: &HashMap<String, Value>,
    init: usize,
    lead: usize,
    pressure: Option<usize>,
) -> Result<Task> {
    let m = &metas[&name];
    let dims = m["dimension_names"]
        .as_array()
        .context("Missing dimensions")?;
    let chunks = m["chunk_grid"]["configuration"]["chunk_shape"]
        .as_array()
        .context("Missing chunks")?;
    let spatial = if dims.iter().any(|d| d == "y") {
        ["y", "x"]
    } else {
        ["latitude", "longitude"]
    };
    let mut expected = vec!["init_time", "lead_time", spatial[0], spatial[1]];
    if pressure.is_some() {
        expected.push(if name.starts_with("model_level/") {
            "model_level"
        } else {
            "pressure_level"
        });
    }
    anyhow::ensure!(
        m["data_type"] == "float64"
            && dims
                .iter()
                .map(|d| d.as_str())
                .eq(expected.iter().map(|d| Some(*d))),
        "Source dimension order/type changed"
    );
    let shape = m["shape"].as_array().context("Missing array shape")?;
    anyhow::ensure!(
        shape.len() == chunks.len() && chunks.len() == dims.len(),
        "Source rank changed"
    );
    let mut coords = vec![];
    for ((d, chunk), size) in dims.iter().zip(chunks).zip(shape) {
        let d = d.as_str().context("Unnamed dimension")?;
        let chunk = chunk.as_u64().context("Bad chunk")? as usize;
        let i = match d {
            "init_time" => init,
            "lead_time" => lead,
            "pressure_level" | "model_level" => pressure.context("Missing level")?,
            "x" | "y" | "latitude" | "longitude" => 0,
            _ => bail!("Unknown dimension {d}"),
        };
        if matches!(
            d,
            "init_time" | "lead_time" | "pressure_level" | "model_level"
        ) {
            anyhow::ensure!(chunk == 1, "Nonspatial chunks changed");
        } else {
            anyhow::ensure!(
                Some(chunk as u64) == size.as_u64(),
                "Spatial chunks no longer cover complete maps"
            );
        }
        coords.push((i / chunk).to_string());
    }
    let key = format!("{name}/c/{}", coords.join("/"));
    Ok(Task {
        field,
        level,
        name,
        meta: m.clone(),
        key,
    })
}

pub struct AvailableRun {
    pub pin: Value,
    pub expected: Vec<u16>,
    pub ready: Vec<u16>,
    pub missing: usize,
    pub cloud_ready: Option<Vec<u16>>,
}
pub struct Availability {
    pub source: Arc<Source>,
    pub runs: Vec<AvailableRun>,
}
pub async fn ready_hours<F, Fut>(
    hours: Vec<(u16, Vec<String>)>,
    exists: F,
) -> Result<Vec<(u16, usize)>>
where
    F: Fn(String) -> Fut,
    Fut: std::future::Future<Output = Result<bool>>,
{
    let mut missing: std::collections::BTreeMap<u16, usize> =
        hours.iter().map(|(h, _)| (*h, 0)).collect();
    let checks = stream::iter(
        hours
            .into_iter()
            .flat_map(|(hour, keys)| keys.into_iter().map(move |key| (hour, key))),
    )
    .map(|(h, k)| {
        let future = exists(k);
        async move { Ok::<_, anyhow::Error>((h, future.await?)) }
    })
    .buffer_unordered(16)
    .try_collect::<Vec<_>>()
    .await?;
    for (hour, present) in checks {
        if !present {
            *missing.get_mut(&hour).unwrap() += 1;
        }
    }
    Ok(missing.into_iter().collect())
}
pub async fn availability(model: &str, product: &str, tracked: &[String]) -> Result<Availability> {
    availability_filtered(model, product, tracked, &[], None).await
}
pub async fn availability_filtered(
    model: &str,
    product: &str,
    tracked: &[String],
    finished: &[String],
    floor: Option<f64>,
) -> Result<Availability> {
    if matches!(model, "ifs" | "aigfs")
        || (model == "aifs" && std::env::var("METEOGRAM_AIFS_SOURCE").as_deref() == Ok("direct"))
    {
        anyhow::ensure!(product == dataset(model)?, "Invalid direct product");
        return direct_availability(model, tracked, finished, floor).await;
    }
    let source = Arc::new(Source::open(model, Some(&json!({"dataset":product}))).await?);
    let times = source.init_times().await?;
    let mut selected = times
        .iter()
        .enumerate()
        .filter(|(_, t)| **t <= now())
        .filter(|(_, t)| {
            !(model == "hrrr" && product == HRRR_SHORT && (**t as i64).rem_euclid(21600) == 0)
        })
        .rev()
        .take(3)
        .map(|(i, t)| (i, *t))
        .collect::<Vec<_>>();
    for value in tracked {
        let t = timestamp(value)?;
        if let Some(i) = times.iter().position(|x| (*x - t).abs() < 0.001) {
            if !selected.iter().any(|(j, _)| *j == i) {
                selected.push((i, t));
            }
        }
    }
    let completed = finished
        .iter()
        .map(|v| timestamp(v))
        .collect::<Result<Vec<_>>>()?;
    selected.retain(|(_, t)| {
        !completed.iter().any(|old| (old - t).abs() < 0.001) && floor.is_none_or(|old| *t > old)
    });
    if selected.is_empty() {
        return Ok(Availability {
            source,
            runs: Vec::new(),
        });
    }
    let lead_values = source.coord("lead_time").await?;
    let lead_meta = source.meta("lead_time").await?;
    let factor = match lead_meta["attributes"]["units"].as_str() {
        Some("seconds") => 1. / 3600.,
        Some("hours") => 1.,
        Some("nanoseconds") => 1. / 3.6e12,
        _ => bail!("Unsupported lead units"),
    };
    let source_leads = lead_values
        .iter()
        .map(|v| (*v * factor).round() as u16)
        .collect::<Vec<_>>();
    let pressures = source.coord("pressure_level/pressure_level").await?;
    let levels = pressure_levels(model);
    let mut metas = HashMap::new();
    for name in variables(model)
        .into_iter()
        .map(|v| format!("pressure_level/{v}"))
        .chain([
            "temperature_2m".to_owned(),
            "dew_point_temperature_2m".to_owned(),
        ])
    {
        metas.insert(name.clone(), source.meta(&name).await?);
    }
    if model == "hrrr" {
        for field in crate::clouds::FIELDS {
            let name = format!("model_level/{field}");
            metas.insert(name.clone(), source.meta(&name).await?);
        }
    }
    let expected = product_leads(model, product);
    let mut runs = Vec::new();
    for (init, t) in selected {
        let mut hours = Vec::new();
        for &hour in &expected {
            let lead = source_leads
                .iter()
                .position(|v| *v == hour)
                .context("Forecast axis changed")?;
            let mut keys = Vec::new();
            for (field, name) in variables(model).iter().enumerate() {
                for (level, p) in levels.iter().enumerate() {
                    let pressure = pressures
                        .iter()
                        .position(|v| *v == f64::from(*p))
                        .context("Pressure axis changed")?;
                    keys.push(
                        task(
                            field,
                            Some(level),
                            format!("pressure_level/{name}"),
                            &metas,
                            init,
                            lead,
                            Some(pressure),
                        )?
                        .key,
                    );
                }
            }
            for (field, name) in ["temperature_2m", "dew_point_temperature_2m"]
                .iter()
                .enumerate()
            {
                keys.push(task(field, None, (*name).to_owned(), &metas, init, lead, None)?.key);
            }
            hours.push((hour, keys));
        }
        let counts = ready_hours(hours, |key| {
            let s = source.store.clone();
            async move { s.exists(&key).await }
        })
        .await?;
        let cloud_ready = if model == "hrrr" {
            let hours = expected
                .iter()
                .map(|hour| {
                    let lead = source_leads
                        .iter()
                        .position(|v| v == hour)
                        .context("Cloud lead axis")?;
                    Ok((
                        *hour,
                        cloud_tasks(&metas, init, lead)?
                            .into_iter()
                            .map(|t| t.key)
                            .collect(),
                    ))
                })
                .collect::<Result<Vec<_>>>()?;
            Some(
                ready_hours(hours, |key| {
                    let store = source.store.clone();
                    async move { store.exists(&key).await }
                })
                .await?
                .into_iter()
                .filter(|(_, n)| *n == 0)
                .map(|(h, _)| h)
                .collect(),
            )
        } else {
            None
        };
        let init_time = chrono::DateTime::from_timestamp(t as i64, 0)
            .context("Invalid init")?
            .format("%Y-%m-%dT%H:%M:%S%.9f")
            .to_string();
        runs.push(AvailableRun {pin:json!({"model":model,"dataset":product,"init_time":init_time,"snapshot_id":source.snapshot}),expected:expected.clone(),ready:counts.iter().filter(|(_,n)|*n==0).map(|(h,_)|*h).collect(),missing:counts.iter().map(|(_,n)|n).sum(),cloud_ready});
    }
    Ok(Availability { source, runs })
}

fn direct_cycles(at: i64) -> Vec<i64> {
    let latest = at.div_euclid(21600) * 21600;
    // The newest two cycles may still be unpublished at rollover. Keep the
    // preceding complete short cycle and 180-hour IFS baseline discoverable.
    (0..4).map(|n| latest - n * 21600).collect()
}
async fn direct_availability(
    model: &str,
    tracked: &[String],
    finished: &[String],
    floor: Option<f64>,
) -> Result<Availability> {
    let latest = (now() as i64).div_euclid(21600) * 21600;
    let mut times = direct_cycles(now() as i64);
    for init in tracked {
        let t = timestamp(init)? as i64;
        if t <= latest {
            times.push(t);
        }
    }
    let complete = finished
        .iter()
        .map(|t| timestamp(t).map(|t| t as i64))
        .collect::<Result<Vec<_>>>()?;
    times.retain(|t| !complete.contains(t) && floor.is_none_or(|f| *t as f64 > f));
    times.sort_unstable();
    times.dedup();
    let direct = crate::ecmwf::discover_model(model, times.clone()).await?;
    let source = Arc::new(Source::from_ecmwf(direct)?);
    let SourceStore::Ecmwf(direct) = &*source.store else {
        unreachable!()
    };
    let runs = direct.available_times().into_iter().rev().map(|init| {
        let ready = direct.hours.iter().filter(|h|h.init == init).map(|h|h.lead).collect::<Vec<_>>();
        let init_time = chrono::DateTime::from_timestamp(init,0).unwrap().format("%Y-%m-%dT%H:%M:%S%.9f").to_string();
        let expected = cycle_leads(model, init);
        AvailableRun {pin:json!({"model":model,"dataset":dataset(model).unwrap(),"init_time":init_time,"snapshot_id":source.snapshot,"source_provider":crate::ecmwf::provider(model)}),missing:(expected.len()-ready.len())*(variables(model).len()*pressure_levels(model).len()+surface_variables(model).len()),expected,ready,cloud_ready:None}
    }).collect();
    Ok(Availability { source, runs })
}

pub async fn probe(model: &str, pin: &Value) -> Result<Value> {
    let s = Source::open(model, Some(pin)).await?;
    let times = s.coord("init_time").await?;
    let time = timestamp(pin["init_time"].as_str().context("Missing init")?)?;
    let init = times
        .iter()
        .position(|v| (*v - time).abs() < 0.001)
        .context("No init")?;
    let pressures = s.coord("pressure_level/pressure_level").await?;
    let pressure = pressures
        .iter()
        .position(|p| *p == 1000.)
        .context("No 1000hPa")?;
    let name = "pressure_level/temperature";
    let m = s.meta(name).await?;
    let metas = HashMap::from([(name.to_owned(), m.clone())]);
    let t = task(0, Some(0), name.to_owned(), &metas, init, 0, Some(pressure))?;
    let coords = t
        .key
        .split("/c/")
        .nth(1)
        .context("No coordinates")?
        .split('/')
        .map(str::parse)
        .collect::<std::result::Result<Vec<u32>, _>>()?;
    let path = icechunk::format::Path::try_from(format!("/{name}").as_str())
        .map_err(anyhow::Error::msg)?;
    let reference = s
        .store
        .icechunk()?
        .session()
        .read()
        .await
        .get_chunk_ref(&path, &icechunk::format::ChunkIndices(coords))
        .await?;
    println!(
        "{}",
        json!({"key":t.key,"reference":format!("{reference:?}")})
    );
    let started = Instant::now();
    let bytes = tokio::time::timeout(
        std::time::Duration::from_secs(30),
        s.store.get(&t.key, &ByteRange::ALL),
    )
    .await??;
    let values = decode(&m, &bytes)?;
    Ok(
        json!({"key":t.key,"bytes":bytes.len(),"values":values.len(),"first_finite":values.iter().copied().find(|v|v.is_finite()),"seconds":started.elapsed().as_secs_f64()}),
    )
}

/// Fetch all 50 HRRR model levels for the same initialized forecast hour.
/// Cloud references can arrive after pressure references; pin their own snapshot.
/// Whole-map reads happen only in the background updater/backfill.
pub async fn fetch_clouds(out: &Path, available: Option<&Source>) -> Result<Value> {
    let mut meta = read_json(out.join("metadata.json"))?;
    anyhow::ensure!(
        meta["model"] == "hrrr" && meta["leads"].as_array().map(Vec::len) == Some(1),
        "Native clouds require one HRRR hour"
    );
    if crate::clouds::binding(out, &meta)?.is_some() {
        return Ok(meta["native_clouds"].clone());
    }
    let opened;
    let src = if let Some(s) = available.filter(|s| s.collection["id"] == meta["dataset"]) {
        s
    } else {
        opened = Source::open("hrrr", Some(&json!({"dataset":meta["dataset"]}))).await?;
        &opened
    };
    let info = fetch_cloud_coverage(out, src).await?;
    meta["native_clouds"] = info.clone();
    atomic(out.join("metadata.json"), &meta)?;
    Ok(info)
}

/// All 50 exact fraction maps plus two small native geometry anchors. No
/// per-level height/pressure maps are retained or downloaded by the point API.
pub async fn fetch_cloud_coverage(out: &Path, available: &Source) -> Result<Value> {
    let started = Instant::now();
    let meta = read_json(out.join("metadata.json"))?;
    anyhow::ensure!(
        meta["model"] == "hrrr"
            && meta["leads"].as_array().map(Vec::len) == Some(1)
            && available.collection["id"] == meta["dataset"],
        "Cloud source identity differs"
    );
    let init_time = timestamp(meta["init_time"].as_str().context("Cloud init")?)?;
    let init = available
        .init_times()
        .await?
        .iter()
        .position(|t| (*t - init_time).abs() < 0.001)
        .context("Cloud initialization unavailable")?;
    let lead_meta = available.meta("lead_time").await?;
    let factor = match lead_meta["attributes"]["units"].as_str() {
        Some("seconds") => 1. / 3600.,
        Some("hours") => 1.,
        Some("nanoseconds") => 1e-9 / 3600.,
        _ => bail!("Unsupported cloud lead units"),
    };
    let lead = available
        .coord("lead_time")
        .await?
        .iter()
        .position(|h| (*h * factor - meta["leads"][0].as_u64().unwrap() as f64).abs() < 0.001)
        .context("Cloud lead unavailable")?;
    let name = "model_level/fraction_of_cloud_cover";
    let pressure_name = "model_level/pressure";
    let height_name = "model_level/geopotential_height";
    let (model_levels, field, pressure_field, height_field) = tokio::try_join!(
        available.coord("model_level/model_level"),
        available.meta(name),
        available.meta(pressure_name),
        available.meta(height_name)
    )?;
    anyhow::ensure!(
        model_levels
            == (1..=crate::cloud_coverage::LEVELS)
                .map(|n| n as f64)
                .collect::<Vec<_>>(),
        "Native cloud level axis differs"
    );
    let xs = available.coord("x").await?;
    let ys = available.coord("y").await?;
    let crs = Lambert::from_cf(&available.meta("spatial_ref").await?["attributes"])?;
    anyhow::ensure!(
        json!(xs) == meta["grid"]["x"]
            && json!(ys) == meta["grid"]["y"]
            && crs.crs() == meta["grid"]["crs"],
        "Cloud grid differs"
    );
    let arrays = crate::stage::load_cells(out)?;
    let array = arrays.get("cell_id").context("Cloud cells")?;
    let ids: Vec<u32> = array
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let native_cells = xs.len() * ys.len();
    anyhow::ensure!(
        array.dtype == "<u4"
            && array.bytes.len().is_multiple_of(4)
            && ids.len() == meta["cells"].as_u64().context("Cloud cell count")? as usize
            && !ids.is_empty()
            && ids.len() <= 2_000_000
            && ids.windows(2).all(|v| v[0] < v[1])
            && ids.last().is_some_and(|id| (*id as usize) < native_cells),
        "Cloud retained cells differ"
    );
    let ids = Arc::new(ids);
    anyhow::ensure!(
        field["attributes"]["units"] == "1"
            && pressure_field["attributes"]["units"] == "Pa"
            && height_field["attributes"]["units"] == "m",
        "Cloud field units differ"
    );
    let metas = HashMap::from([
        (name.to_owned(), field),
        (pressure_name.to_owned(), pressure_field),
        (height_name.to_owned(), height_field),
    ]);
    let tasks = cloud_tasks(&metas, init, lead)?;
    let keys = tasks.iter().map(|t| t.key.clone()).collect::<Vec<_>>();
    let setup_seconds = started.elapsed().as_secs_f64();
    let preflight_started = Instant::now();
    let preflight = check_references(keys.clone(), |key| {
        let store = available.store.clone();
        async move { store.exists(&key).await }
    })
    .await?;
    if preflight["complete"] != true {
        return Err(SourcePending("Cloud").into());
    }
    let preflight_seconds = preflight_started.elapsed().as_secs_f64();
    let cache = cloud_cache_dir(out, &available.snapshot)?;
    let count = Arc::new(AtomicU64::new(0));
    enum CloudValues {
        Coverage(crate::cloud_coverage::Level),
        Anchor(Vec<f32>),
        Top,
    }
    let mut reads = stream::iter(tasks).map(|t| {
        let store = available.store.clone();
        let cache = cache.clone();
        let ids = ids.clone();
        let count = count.clone();
        let slots = available.map_slots.clone();
        async move {
            let _permit = slots.acquire().await?;
            let read_started = Instant::now();
            let bytes = cached(&store, &cache, &t.key, &count).await?;
            let read_seconds = read_started.elapsed().as_secs_f64();
            tokio::task::spawn_blocking(move || -> Result<_> {
                let decode_started = Instant::now();
                let map = decode(&t.meta, &bytes)?;
                anyhow::ensure!(map.len() == native_cells, "Cloud map shape differs");
                let decode_seconds = decode_started.elapsed().as_secs_f64();
                let encode_started = Instant::now();
                let gathered = ids.iter().map(|id| map[*id as usize] as f32);
                let values = match t.field {
                    0 => CloudValues::Coverage(crate::cloud_coverage::encode(gathered)?),
                    1 | 2 => CloudValues::Anchor(gathered.collect()),
                    3 => {
                        anyhow::ensure!(
                            gathered.clone().all(|p| p.is_finite() && (f64::from(p) - crate::cloud_geometry::top_pressure()).abs() < 0.1),
                            "HRRR hybrid model top changed"
                        );
                        CloudValues::Top
                    }
                    _ => anyhow::bail!("Unknown cloud field"),
                };
                let timing = json!({"field":t.field,"level":t.level,"read_seconds":read_seconds,"decode_seconds":decode_seconds,"encode_seconds":encode_started.elapsed().as_secs_f64(),"bytes":bytes.len()});
                Ok((t.field, t.level.context("Cloud level")?, values, timing))
            }).await?
        }
    }).buffer_unordered(16);
    let mut levels = (0..crate::cloud_coverage::LEVELS)
        .map(|_| None)
        .collect::<Vec<_>>();
    let mut anchors: [Option<Vec<f32>>; 2] = [None, None];
    let mut top_checked = false;
    let mut error = None;
    let mut map_timings = Vec::new();
    while let Some(result) = reads.next().await {
        match result {
            Ok((field, level, values, timing)) if error.is_none() => {
                let duplicate = match values {
                    CloudValues::Coverage(codes) => levels[level].replace(codes).is_some(),
                    CloudValues::Anchor(values) => anchors[field - 1].replace(values).is_some(),
                    CloudValues::Top => std::mem::replace(&mut top_checked, true),
                };
                if duplicate {
                    error = Some(anyhow::anyhow!("Duplicate cloud source map"));
                }
                map_timings.push(timing);
            }
            Err(e) if error.is_none() => error = Some(e),
            _ => {}
        }
    }
    if let Some(e) = error {
        return Err(e);
    }
    anyhow::ensure!(top_checked, "Missing source model-top check");
    let acquisition_seconds = started.elapsed().as_secs_f64();
    let levels = levels
        .into_iter()
        .map(|l| l.context("Missing cloud level"))
        .collect::<Result<Vec<_>>>()?;
    // Cached retries must preserve the same reserve as fresh downloads. Cover
    // both cloud data/index pairs and their provenance before the first write.
    disk_guard(out, crate::clouds::max_stored_bytes(ids.len())?)?;
    let mut coverage = crate::cloud_coverage::write(out, &levels)?;
    coverage["index_sha256"] = json!(crate::dictionary::file_hash(
        &out.join(crate::cloud_coverage::INDEX)
    )?);
    let [bottom, top] = anchors;
    let anchor_stats = crate::clouds::write_anchors(
        out,
        &[
            bottom.context("Missing bottom pressure")?,
            top.context("Missing top height")?,
        ],
    )?;
    coverage["data_sha256"] = json!(crate::dictionary::file_hash(
        &out.join(crate::cloud_coverage::DATA)
    )?);
    let mut info = crate::clouds::metadata(&meta, &available.snapshot, coverage, anchor_stats)?;
    info["source_maps"] = json!(53);
    info["source_top_checked"] = json!(true);
    info["setup_seconds"] = json!(setup_seconds);
    info["preflight_seconds"] = json!(preflight_seconds);
    info["map_timings"] = json!(map_timings);
    info["acquisition_seconds"] = json!(acquisition_seconds);
    info["grib_bytes"] = json!(count.load(Ordering::Relaxed));
    atomic(out.join("cloud-coverage-provenance.json"), &info)?;
    prune_checkpointed_cache(&cache, &keys)?;
    info["total_seconds"] = json!(started.elapsed().as_secs_f64());
    atomic(out.join("cloud-coverage-provenance.json"), &info)?;
    Ok(info)
}

fn cloud_tasks(metas: &HashMap<String, Value>, init: usize, lead: usize) -> Result<Vec<Task>> {
    let name = "model_level/fraction_of_cloud_cover";
    let pressure_name = "model_level/pressure";
    let height_name = "model_level/geopotential_height";
    let mut tasks = (0..crate::cloud_coverage::LEVELS)
        .map(|l| task(0, Some(l), name.to_owned(), metas, init, lead, Some(l)))
        .collect::<Result<Vec<_>>>()?;
    tasks.push(task(
        1,
        Some(0),
        pressure_name.to_owned(),
        metas,
        init,
        lead,
        Some(0),
    )?);
    tasks.push(task(
        2,
        Some(49),
        height_name.to_owned(),
        metas,
        init,
        lead,
        Some(49),
    )?);
    // Confirm the configured hybrid lid against this immutable source hour.
    tasks.push(task(
        3,
        Some(49),
        pressure_name.to_owned(),
        metas,
        init,
        lead,
        Some(49),
    )?);
    Ok(tasks)
}

pub async fn cloud_availability(product: &str) -> Result<Value> {
    let src = Source::open("hrrr", Some(&json!({"dataset":product}))).await?;
    let inits = src.init_times().await?;
    let mut metas = HashMap::new();
    for name in crate::clouds::FIELDS {
        let name = format!("model_level/{name}");
        metas.insert(name.clone(), src.meta(&name).await?);
    }
    let mut runs = Vec::new();
    for init in (inits.len().saturating_sub(24)..inits.len()).rev() {
        let mut hours = Vec::new();
        for hour in [0, 1, 12, 18] {
            let keys = cloud_tasks(&metas, init, hour)?
                .into_iter()
                .map(|t| t.key)
                .collect();
            let result = check_references(keys, |key| {
                let store = src.store.clone();
                async move { store.exists(&key).await }
            })
            .await?;
            hours.push(json!({"lead":hour,"references":result}));
        }
        runs.push(json!({"init_time":chrono::DateTime::from_timestamp(inits[init] as i64,0).context("Init")?.to_rfc3339(),"hours":hours}));
    }
    Ok(json!({"dataset":product,"snapshot_id":src.snapshot,"runs":runs,"payload_reads":0}))
}

#[cfg(test)]
mod cloud_tests {
    use super::*;
    #[test]
    fn cloud_retries_are_snapshot_scoped_and_preserve_pressure_cache() {
        let root = tempfile::tempdir().unwrap();
        let pressure = root.path().join("source-cache/pressure");
        fs::create_dir_all(pressure.parent().unwrap()).unwrap();
        fs::write(&pressure, b"pressure").unwrap();
        let first = cloud_cache_dir(root.path(), "snapshot-a").unwrap();
        fs::write(first.join("encoded"), b"cloud-a").unwrap();
        assert!(cloud_cache_dir(root.path(), "snapshot-a")
            .unwrap()
            .join("encoded")
            .exists());
        let next = cloud_cache_dir(root.path(), "snapshot-b").unwrap();
        assert_ne!(first, next);
        assert!(!first.exists());
        assert!(next.is_dir());
        assert_eq!(fs::read(pressure).unwrap(), b"pressure");
    }
    #[test]
    fn model_level_maps_require_the_declared_axis_and_complete_spatial_chunks() {
        let name = "model_level/fraction_of_cloud_cover".to_owned();
        let meta = json!({"data_type":"float64","dimension_names":["init_time","lead_time","y","x","model_level"],"shape":[2,19,10,20,50],"chunk_grid":{"configuration":{"chunk_shape":[1,1,10,20,1]}}});
        let mut metas = HashMap::from([(name.clone(), meta)]);
        let t = task(0, Some(49), name.clone(), &metas, 1, 2, Some(49)).unwrap();
        assert_eq!(t.key, "model_level/fraction_of_cloud_cover/c/1/2/0/0/49");
        metas.get_mut(&name).unwrap()["dimension_names"][4] = json!("pressure_level");
        assert!(task(0, Some(0), name.clone(), &metas, 1, 2, Some(0)).is_err());
        metas.get_mut(&name).unwrap()["dimension_names"][4] = json!("model_level");
        metas.get_mut(&name).unwrap()["chunk_grid"]["configuration"]["chunk_shape"][4] = json!(2);
        assert!(task(0, Some(0), name, &metas, 1, 2, Some(0)).is_err());
    }
}

#[cfg(test)]
mod ecmwf_tests {
    use super::*;
    #[test]
    #[ignore = "Requires ECMWF_TEST_STAGE from live acquisition and loopback ClickHouse"]
    fn direct_ecmwf_staged_database_roundtrip() -> Result<()> {
        let stage =
            PathBuf::from(std::env::var("ECMWF_TEST_STAGE").context("Set ECMWF_TEST_STAGE")?);
        check_database_roundtrip(&stage)
    }
    fn check_database_roundtrip(stage: &Path) -> Result<()> {
        let metadata = read_json(stage.join("metadata.json"))?;
        let model = metadata["model"].as_str().context("Model")?;
        let root = tempfile::tempdir()?;
        let (path, entry) = candidate(&root.path().join("incoming"), model)?;
        for file in fs::read_dir(stage)? {
            let file = file?;
            if file.file_type()?.is_file() {
                fs::copy(file.path(), path.join(file.file_name()))?;
            }
        }
        let metadata = read_json(path.join("metadata.json"))?;
        crate::ecmwf::check_metadata(&metadata)?;
        let lead = metadata["leads"][0].as_u64().context("Missing lead")? as u16;
        let shared = format!("rust_test_ecmwf_{}", uuid::Uuid::new_v4().simple());
        let db = crate::db::Db::new()?;
        let result = (|| -> Result<()> {
            crate::store::load_lead(root.path(), &path, &entry.table, lead)?;
            let proof = crate::store::validate(&path, &entry.table, DATABASE, true)?;
            anyhow::ensure!(
                proof["full_values_checked"] == true && proof["staged_comparison"] == true,
                "Incomplete database comparison"
            );
            let first =
                crate::incremental::attach_lead(root.path(), model, &path, &entry, &shared, None)?;
            let again =
                crate::incremental::attach_lead(root.path(), model, &path, &entry, &shared, None)?;
            anyhow::ensure!(first == again, "Duplicate attachment changed publication");
            let (status, payload) = crate::api::test_forecast(
                root.path(),
                [("model", model), ("lat", "41"), ("lon", "-74")]
                    .into_iter()
                    .map(|(k, v)| (k.to_owned(), v.to_owned()))
                    .collect(),
            );
            anyhow::ensure!(
                status == axum::http::StatusCode::OK
                    && payload["source_providers"]
                        == json!([if model == "aigfs" { "noaa" } else { "ecmwf" }])
                    && payload["available_leads"] == json!([lead]),
                "Direct API provenance/forecast differs: {payload}"
            );
            println!(
                "{}",
                json!({"direct_database_roundtrip":true,"lead":lead,"full_values_checked":true,"staged_comparison":true,"idempotent_publication":true,"source_providers":payload["source_providers"]})
            );
            Ok(())
        })();
        for table in [&entry.table, &shared] {
            db.command(&format!(
                "DROP TABLE IF EXISTS {DATABASE}.{} SYNC",
                ident(table)?
            ))?;
        }
        result
    }
    #[tokio::test]
    #[ignore = "Downloads live ECMWF and Dynamical AIFS; requires explicit initialization and output directory"]
    async fn direct_ecmwf_live_exactness_and_resume() -> Result<()> {
        let init = std::env::var("ECMWF_TEST_INIT").context("Set ECMWF_TEST_INIT")?;
        let output =
            PathBuf::from(std::env::var("ECMWF_TEST_OUTPUT").context("Set ECMWF_TEST_OUTPUT")?);
        anyhow::ensure!(!output.exists(), "Use a fresh output directory");
        fs::create_dir_all(&output)?;
        let source = Source::open("aifs", None).await?;
        let direct =
            Source::from_ecmwf(crate::ecmwf::discover(vec![timestamp(&init)? as i64]).await?)?;
        let SourceStore::Ecmwf(d) = &*direct.store else {
            unreachable!()
        };
        anyhow::ensure!(d.hours.len() == 31, "Direct run is not complete");
        let pin = json!({"model":"aifs","dataset":dataset("aifs")?,"init_time":init});
        let mut reports = Vec::new();
        for lead in [0, 102] {
            let reference_dir = output.join(format!("dynamical-{lead}"));
            let direct_dir = output.join(format!("direct-{lead}"));
            let started = Instant::now();
            let reference =
                fetch_pressure_hour("aifs", &reference_dir, pin.clone(), lead, &source).await?;
            let reference_seconds = started.elapsed().as_secs_f64();
            let started = Instant::now();
            let actual =
                fetch_pressure_hour("aifs", &direct_dir, pin.clone(), lead, &direct).await?;
            let direct_seconds = started.elapsed().as_secs_f64();
            for key in [
                "cells",
                "levels",
                "variables",
                "surface_variables",
                "grid",
                "units",
            ] {
                anyhow::ensure!(
                    reference[key] == actual[key],
                    "Source schema differs: {key}"
                );
            }
            let cells = crate::stage::load_cells(&reference_dir)?;
            let other = crate::stage::load_cells(&direct_dir)?;
            for (name, array) in cells {
                anyhow::ensure!(array.bytes == other[&name].bytes, "Coordinates/IDs differ");
            }
            let a = crate::dictionary::Dictionary::load(&reference_dir, &reference)?;
            let b = crate::dictionary::Dictionary::load(&direct_dir, &actual)?;
            anyhow::ensure!(a.fields == b.fields, "Float32 palette bits differ");
            for name in [format!("lead-{lead:03}"), format!("surface-{lead:03}")] {
                let nc = actual["cells"].as_u64().unwrap() as usize;
                let shape = if name.starts_with("lead-") {
                    vec![nc, 8, 5]
                } else {
                    vec![nc, 2]
                };
                let a =
                    crate::stage::Array::open_as(&reference_dir.join(&name), &shape, "uint16_le")?;
                let b = crate::stage::Array::open_as(&direct_dir.join(&name), &shape, "uint16_le")?;
                let mut av = vec![0.; shape.iter().product()];
                let mut bv = vec![0.; shape.iter().product()];
                a.read_into(0, &mut av)?;
                b.read_into(0, &mut bv)?;
                anyhow::ensure!(av == bv, "Staged spatial indices differ");
            }
            let resumed =
                fetch_pressure_hour("aifs", &direct_dir, pin.clone(), lead, &direct).await?;
            anyhow::ensure!(resumed == actual, "Resuming changed hourly metadata");
            let values = actual["cells"].as_u64().unwrap() * 42;
            reports.push(json!({"lead":lead,"compared_values":values,"all_bits_equal":true,"reference_seconds":reference_seconds,"direct_seconds":direct_seconds,"resume_equal":true}));
        }
        let path = output.join("direct-180");
        let actual = fetch_pressure_hour("aifs", &path, pin, 180, &direct).await?;
        anyhow::ensure!(
            actual["staging"]["complete"] == true
                && actual["source_payload_sha256"].as_object().unwrap().len() == 42,
            "Missing direct source proof"
        );
        let report = json!({"init_time":init,"source_ready_leads":31,"comparisons":reports,"previously_missing_lead":180,"missing_lead_staged":true,"payload_proofs":42});
        atomic(output.join("verification.json"), &report)?;
        println!("{report}");
        Ok(())
    }
    #[test]
    fn direct_cycle_rollover_keeps_the_previous_long_baseline_discoverable() {
        let at = timestamp("2026-10-21T18:01:00").unwrap() as i64;
        let cycles = direct_cycles(at);
        assert_eq!(cycles.len(), 4);
        assert!(cycles.contains(&(timestamp("2026-10-21T00:00:00").unwrap() as i64)));
        assert!(cycles.contains(&(timestamp("2026-10-21T06:00:00").unwrap() as i64)));
        assert_eq!(cycle_leads("ifs", cycles[3]).last(), Some(&180));
    }
    #[tokio::test]
    #[ignore = "Requires DIRECT_TEST_INIT, fresh DIRECT_TEST_OUTPUT, network and loopback ClickHouse"]
    async fn new_global_sources_live_full_grid_and_database_roundtrip() -> Result<()> {
        let init = std::env::var("DIRECT_TEST_INIT").context("Set DIRECT_TEST_INIT")?;
        let output =
            PathBuf::from(std::env::var("DIRECT_TEST_OUTPUT").context("Set DIRECT_TEST_OUTPUT")?);
        anyhow::ensure!(!output.exists(), "Use a fresh output directory");
        fs::create_dir_all(&output)?;
        let mut reports = Vec::new();
        for model in ["ifs", "aigfs"] {
            for lead in [0, 180] {
                let direct = Source::from_ecmwf(
                    crate::ecmwf::single_hour(model, timestamp(&init)? as i64, lead).await?,
                )?;
                let pin = json!({"model":model,"dataset":dataset(model)?,"init_time":init});
                let path = output.join(format!("{model}-{lead}"));
                let start = Instant::now();
                let metadata =
                    fetch_pressure_hour(model, &path, pin.clone(), lead, &direct).await?;
                let seconds = start.elapsed().as_secs_f64();
                let cells = crate::stage::load_cells(&path)?;
                let ids = cells["cell_id"]
                    .bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| u32::from_le_bytes(*b) as usize)
                    .collect::<Vec<_>>();
                let nf = variables(model).len();
                let nl = pressure_levels(model).len();
                let ns = surface_variables(model).len();
                anyhow::ensure!(ids.len() == 24885, "CONUS cells were lost");
                let dictionary = crate::dictionary::Dictionary::load(&path, &metadata)?;
                let mut profiles = vec![0f32; ids.len() * nf * nl];
                let mut surfaces = vec![0f32; ids.len() * ns];
                crate::stage::Array::open_as(
                    &path.join(format!("lead-{lead:03}")),
                    &[ids.len(), nl, nf],
                    "uint16_le",
                )?
                .read_into(0, &mut profiles)?;
                crate::stage::Array::open_as(
                    &path.join(format!("surface-{lead:03}")),
                    &[ids.len(), ns],
                    "uint16_le",
                )?
                .read_into(0, &mut surfaces)?;
                let SourceStore::Ecmwf(source) = &*direct.store else {
                    unreachable!()
                };
                let ti = leads(model)
                    .iter()
                    .position(|h| *h == lead)
                    .context("Lead index")?;
                let results = stream::iter(&source.hours[0].fields)
                    .map(|field| {
                        let ids = &ids;
                        let profiles = &profiles;
                        let surfaces = &surfaces;
                        let dictionary = &dictionary;
                        async move {
                            let pressure = field.pressure.map(|p| {
                                pressure_levels(model).iter().position(|v| *v == p).unwrap()
                            });
                            let key = format!(
                                "{}/c/0/{ti}/0/0{}",
                                field.name,
                                pressure.map_or(String::new(), |p| format!("/{p}"))
                            );
                            let bytes = source.get(&key).await?;
                            let values = decode(&source.meta(&field.name)?, &bytes)?;
                            let f = if pressure.is_some() {
                                variables(model)
                                    .iter()
                                    .position(|v| format!("pressure_level/{v}") == field.name)
                                    .unwrap()
                            } else {
                                nf + surface_variables(model)
                                    .iter()
                                    .position(|v| *v == field.name)
                                    .unwrap()
                            };
                            for (cell, id) in ids.iter().enumerate() {
                                let code = if let Some(l) = pressure {
                                    profiles[(cell * nl + l) * nf + f]
                                } else {
                                    surfaces[cell * ns + f - nf]
                                };
                                anyhow::ensure!(
                                    dictionary.value(f, pressure.unwrap_or(0), code.to_bits())?
                                        == (values[*id] as f32).to_bits(),
                                    "Source/staging bit mismatch {model}/{lead}/{} cell {cell}",
                                    field.name
                                );
                            }
                            Ok::<_, anyhow::Error>(ids.len())
                        }
                    })
                    .buffer_unordered(4)
                    .try_collect::<Vec<_>>()
                    .await?;
                let resumed = fetch_pressure_hour(model, &path, pin, lead, &direct).await?;
                anyhow::ensure!(resumed == metadata, "Resume changed pinned hour");
                let database_path = path.clone();
                tokio::task::spawn_blocking(move || check_database_roundtrip(&database_path))
                    .await??;
                reports.push(json!({"model":model,"lead":lead,"cells":ids.len(),"compared_values":results.iter().sum::<usize>(),"all_source_bits_equal":true,"full_database_validation":true,"resume_equal":true,"source_seconds":seconds,"source_bytes":read_json(path.join(format!("lead-{lead:03}.json")))?["grib_bytes"]}));
            }
        }
        let report = json!({"init_time":init,"comparisons":reports});
        atomic(output.join("verification.json"), &report)?;
        println!("{report}");
        Ok(())
    }
}
