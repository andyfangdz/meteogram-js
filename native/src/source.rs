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
pub struct Source {
    pub store: Arc<Store>,
    pub snapshot: String,
    pub collection: Value,
}
static REPOSITORIES: std::sync::OnceLock<tokio::sync::Mutex<HashMap<String, Arc<Repository>>>> =
    std::sync::OnceLock::new();
impl Source {
    pub async fn open(model: &str, pin: Option<&Value>) -> Result<Self> {
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
            store,
            snapshot,
            collection,
        })
    }
    pub async fn meta(&self, name: &str) -> Result<Value> {
        let bytes = self
            .store
            .get(&format!("{name}/zarr.json"), &ByteRange::ALL)
            .await?;
        Ok(serde_json::from_slice(&bytes)?)
    }
    pub async fn coord(&self, name: &str) -> Result<Vec<f64>> {
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
    if model == "rrfs" {
        // RRFS persists a one-million-reference cache, which thrashes when
        // cycling through the eight required arrays. Keep a bounded manifest
        // cache, without caching weather payloads. Set this at initial open:
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
                fs::remove_file(path)?;
                removed += metadata.len();
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(removed)
}
async fn cached(store: &Store, dir: &Path, key: &str, bytes: &AtomicU64) -> Result<Vec<u8>> {
    let p = cache_path(dir, key);
    if p.exists() {
        return Ok(fs::read(p)?);
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
                bytes.fetch_add(b.len() as u64, Ordering::Relaxed);
                return Ok(b.to_vec());
            }
            Err(e) => {
                last = Some(e);
                tokio::time::sleep(std::time::Duration::from_secs(1 << attempt)).await;
            }
        }
    }
    bail!("Missing source chunk {key}: {:?}", last)
}
pub async fn fetch(
    model: &str,
    out: &Path,
    offset: usize,
    pin: Option<Value>,
    metadata_only: bool,
    selected_leads: Option<Vec<u16>>,
) -> Result<Value> {
    fetch_with(
        model,
        out,
        offset,
        pin,
        metadata_only,
        selected_leads,
        FetchOptions::default(),
    )
    .await
}
#[derive(Default)]
pub struct FetchOptions<'a> {
    pub source: Option<&'a Source>,
    pub dictionary: bool,
}
pub async fn fetch_with(
    model: &str,
    out: &Path,
    offset: usize,
    pin: Option<Value>,
    metadata_only: bool,
    selected_leads: Option<Vec<u16>>,
    options: FetchOptions<'_>,
) -> Result<Value> {
    fs::create_dir_all(out)?;
    anyhow::ensure!(offset > 0, "Run offset must be positive");
    let _lock = Lock::acquire(out, "fetch", true, true)?;
    let start = Instant::now();
    let existing = out.join("metadata.json").exists();
    let old = if existing {
        Some(read_json(out.join("metadata.json"))?)
    } else {
        pin
    };
    // Preserve the physical encoding when resuming, including legacy raw runs.
    let dictionary_mode = if existing {
        old.as_ref().is_some_and(crate::dictionary::staged)
    } else {
        options.dictionary || grid_relative(model)
    };
    if let Some(ref p) = old {
        anyhow::ensure!(
            p["model"] == model
                && p["dataset"]
                    .as_str()
                    .is_some_and(|d| valid_dataset(model, d)),
            "Pinned metadata belongs to another model"
        );
    }
    let opened;
    let src = if let Some(src) = options.source {
        src
    } else {
        opened = Source::open(model, old.as_ref()).await?;
        &opened
    };
    let init_seconds = src.init_times().await?;
    let init_index = if let Some(ref p) = old {
        let s = timestamp(p["init_time"].as_str().context("Missing pinned init")?)?;
        init_seconds
            .iter()
            .position(|v| (*v - s).abs() < 0.001)
            .context("Pinned init unavailable")?
    } else {
        init_seconds
            .len()
            .checked_sub(offset)
            .context("Run offset out of range")?
    };
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
    let surface = ["temperature_2m", "dew_point_temperature_2m"];
    let mut metas = HashMap::new();
    let mut units = serde_json::Map::new();
    for v in &variables {
        let name = format!("pressure_level/{v}");
        let m = src.meta(&name).await?;
        units.insert(v.clone(), m["attributes"]["units"].clone());
        metas.insert(name, m);
    }
    for v in surface {
        metas.insert(v.to_owned(), src.meta(v).await?);
    }
    let product = src.collection["id"]
        .as_str()
        .context("Missing collection id")?;
    anyhow::ensure!(valid_dataset(model, product), "Collection identity differs");
    let leads = selected_leads.unwrap_or_else(|| product_leads(model, product));
    let mut meta = json!({"model":model,"dataset":product,"init_time":init_time,"snapshot_id":src.snapshot,"cells":ids.len(),"levels":levels,"leads":leads,"variables":variables,"surface_variables":surface,"grid":grid,"units":units,"coverage":"conus","spatial_shards":1,"shard_index":0,"workers":16,"zarr_concurrency":16,"open_seconds":start.elapsed().as_secs_f64(),"offline_cache":false});
    if dictionary_mode {
        meta["staging"] = json!({"encoding":crate::dictionary::ENCODING,"complete":false});
    }
    if let Some(ref old) = old {
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
    if metadata_only {
        return Ok(meta);
    }
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
        async move { Ok(store.exists(&key).await?) }
    })
    .await?;
    atomic(out.join("preflight.json"), &references)?;
    anyhow::ensure!(
        references["complete"] == true,
        "Incomplete source references: {}",
        references
    );
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
                crate::stage::exists(&out.join(format!("lead-{lead:03}.npy")))
                    && crate::stage::exists(&out.join(format!("surface-{lead:03}.npy"))),
                "Completed arrays removed; use fresh extraction"
            );
            if dictionary_mode {
                let checkpoint = out.join(format!("lead-{lead:03}.palette"));
                let partial_meta = read_json(checkpoint.join("metadata.json"))?;
                anyhow::ensure!(
                    partial_meta["snapshot_id"] == meta["snapshot_id"]
                        && partial_meta["init_time"] == meta["init_time"]
                        && partial_meta["leads"] == json!([lead]),
                    "Palette checkpoint identity"
                );
                let partial = crate::dictionary::Dictionary::load(&checkpoint, &partial_meta)?
                    .context("Missing palette checkpoint")?;
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
                async move {
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
                        let (values, palette) = if dictionary_mode {
                            let (codes, palette) = crate::dictionary::encode_spatial(values)?;
                            (codes, Some(palette))
                        } else {
                            (values, None)
                        };
                        Ok((t, values, palette))
                    })
                    .await?
                }
            })
            .buffer_unordered(16)
            .try_collect::<Vec<_>>()
            .await?;
        let nc = ids.len();
        let nl = levels.len();
        let nf = variables.len();
        let mut profiles = vec![0f32; nc * nl * nf];
        let mut ground = vec![0f32; nc * 2];
        let mut partial_fields: Vec<Vec<Vec<u32>>> = (0..nf + 2)
            .map(|f| {
                (0..if f < nf { nl } else { 1 })
                    .map(|_| Vec::new())
                    .collect()
            })
            .collect();
        for (t, values, palette) in results {
            if let Some(palette) = palette {
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
                    ground[i * 2 + t.field] = v;
                }
            }
        }
        disk_guard(out, ((profiles.len() + ground.len()) * 4) as u64)?;
        let write = if dictionary_mode {
            crate::stage::write_codes
        } else {
            crate::stage::write
        };
        let profile_stage = write(
            &out.join(format!("lead-{lead:03}.npy")),
            &[nc, nl, nf],
            &profiles,
        )?;
        let surface_stage = write(
            &out.join(format!("surface-{lead:03}.npy")),
            &[nc, 2],
            &ground,
        )?;
        if dictionary_mode {
            let checkpoint = out.join(format!("lead-{lead:03}.palette"));
            fs::create_dir_all(&checkpoint)?;
            let mut partial_meta = meta.clone();
            partial_meta["leads"] = json!([lead]);
            let partial = crate::dictionary::Dictionary::new(partial_fields);
            let info = partial.save(&checkpoint, &partial_meta)?;
            partial_meta["staging"] =
                json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
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
        let stats = json!({"lead":lead,"seconds":started.elapsed().as_secs_f64(),"grib_bytes":bytes.load(Ordering::Relaxed)-before,"chunks":nl*nf+2,"staged_value_bytes":(profiles.len()+ground.len())*4,"finite":true,"profile_stage":profile_stage,"surface_stage":surface_stage,"source_cache_policy":"uncheckpointed_leads_only"});
        atomic(&report, &stats)?;
        // The synced staged arrays and durable checkpoint now replace these
        // encoded maps for resume. Retain only a partially acquired lead.
        prune_checkpointed_cache(&cache, &completed_keys)?;
        println!("{stats}");
    }
    if dictionary_mode {
        let dictionary = crate::dictionary::Dictionary::new(palettes);
        let info = dictionary.save(out, &meta)?;
        meta["staging"] =
            json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
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
        expected.push("pressure_level");
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
            "pressure_level" => pressure.context("Missing level")?,
            "x" | "y" | "latitude" | "longitude" => 0,
            _ => bail!("Unknown dimension {d}"),
        };
        if matches!(d, "init_time" | "lead_time" | "pressure_level") {
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
            async move { Ok(s.exists(&key).await?) }
        })
        .await?;
        let init_time = chrono::DateTime::from_timestamp(t as i64, 0)
            .context("Invalid init")?
            .format("%Y-%m-%dT%H:%M:%S%.9f")
            .to_string();
        runs.push(AvailableRun {pin:json!({"model":model,"dataset":product,"init_time":init_time,"snapshot_id":source.snapshot}),expected:expected.clone(),ready:counts.iter().filter(|(_,n)|*n==0).map(|(h,_)|*h).collect(),missing:counts.iter().map(|(_,n)|n).sum()});
    }
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
