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
    io::Write,
    path::Path,
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
impl Source {
    pub async fn open(model: &str, pin: Option<&Value>) -> Result<Self> {
        let collection: Value = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(60))
            .build()?
            .get(format!(
                "https://stac.dynamical.org/{}/collection.json",
                dataset(model)?
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
        let storage = new_http_storage(href, None, None)?;
        let repo = Repository::open(storage)
            .authorize_virtual_chunk_access(creds.clone())
            .execute()
            .await?;
        // Icechunk 2.3's object_store S3 backend applies S3Options after
        // credentials, so its persisted anonymous=false overrides Anonymous.
        // Preserve explicit public access for the STAC-authorized containers;
        // never fall back to host AWS credentials or EC2 metadata.
        let mut config = repo.config().clone();
        public_config(&mut config, &creds);
        let repo = repo.reopen(Some(config), None).await?;
        let version = if let Some(p) = pin {
            VersionInfo::SnapshotId(
                p["snapshot_id"]
                    .as_str()
                    .context("Missing snapshot")?
                    .try_into()
                    .map_err(anyhow::Error::msg)?,
            )
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
async fn cached(store: &Store, dir: &Path, key: &str, bytes: &AtomicU64) -> Result<Vec<u8>> {
    let digest = format!("{:x}", Sha256::digest(key));
    let p = dir.join(&digest[..2]).join(&digest);
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
    if let Some(ref p) = old {
        anyhow::ensure!(
            p["model"] == model && p["dataset"] == dataset(model)?,
            "Pinned metadata belongs to another model"
        );
    }
    let src = Source::open(model, old.as_ref()).await?;
    let init_values = src.coord("init_time").await?;
    let init_meta = src.meta("init_time").await?;
    let units = init_meta["attributes"]["units"]
        .as_str()
        .context("Missing init units")?;
    let unit_seconds = if units.starts_with("seconds since ") {
        1.
    } else if units.starts_with("hours since ") {
        3600.
    } else if units.starts_with("nanoseconds since ") {
        1e-9
    } else {
        bail!("Unsupported init time units {units}")
    };
    let epoch = units
        .split(" since ")
        .nth(1)
        .context("Missing epoch")?
        .replace(' ', "T");
    let epoch = if epoch.len() == 10 {
        format!("{epoch}T00:00:00")
    } else {
        epoch
    };
    let epoch = timestamp(&epoch)?;
    let init_seconds: Vec<f64> = init_values
        .iter()
        .map(|v| epoch + *v * unit_seconds)
        .collect();
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
    let grid = if model == "hrrr" {
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
    let leads = selected_leads.unwrap_or_else(|| leads(model));
    let meta = json!({"model":model,"dataset":dataset(model)?,"init_time":init_time,"snapshot_id":src.snapshot,"cells":ids.len(),"levels":levels,"leads":leads,"variables":variables,"surface_variables":surface,"grid":grid,"units":units,"coverage":"conus","spatial_shards":1,"shard_index":0,"workers":16,"zarr_concurrency":16,"open_seconds":start.elapsed().as_secs_f64(),"offline_cache":false});
    if let Some(ref old) = old {
        for k in ["init_time", "snapshot_id", "cells", "levels"] {
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
    save_cells(&out.join("cells.npz"), &ids, &lat, &lon)?;
    if metadata_only {
        return Ok(meta);
    }
    let cache = out.join("source-cache");
    fs::create_dir_all(&cache)?;
    let ids = Arc::new(ids);
    let bytes = Arc::new(AtomicU64::new(0));
    for lead in leads {
        let report = out.join(format!("lead-{lead:03}.json"));
        if report.exists() {
            anyhow::ensure!(
                out.join(format!("lead-{lead:03}.npy")).exists()
                    && out.join(format!("surface-{lead:03}.npy")).exists(),
                "Completed arrays removed; use fresh extraction"
            );
            continue;
        }
        let before = bytes.load(Ordering::Relaxed);
        let started = Instant::now();
        let ti = source_leads
            .iter()
            .position(|v| *v == lead)
            .context("Lead unavailable")?;
        let mut tasks = vec![];
        for (field, v) in variables.iter().enumerate() {
            for (level, p) in levels.iter().enumerate() {
                let name = format!("pressure_level/{v}");
                let pi = pressure
                    .iter()
                    .position(|x| *x == *p as f64)
                    .context("Pressure unavailable")?;
                tasks.push(task(
                    field,
                    Some(level),
                    name,
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
                        Ok((t, values))
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
        for (t, values) in results {
            for (i, v) in values.into_iter().enumerate() {
                if let Some(l) = t.level {
                    profiles[(i * nl + l) * nf + t.field] = v;
                } else {
                    ground[i * 2 + t.field] = v;
                }
            }
        }
        disk_guard(out, ((profiles.len() + ground.len()) * 4) as u64)?;
        let raw = |v: &[f32]| -> &[u8] {
            unsafe { std::slice::from_raw_parts(v.as_ptr() as *const u8, v.len() * 4) }
        };
        write_stage(
            out.join(format!("lead-{lead:03}.npy")),
            npy_bytes("<f4", &[nc, nl, nf], raw(&profiles)),
        )?;
        write_stage(
            out.join(format!("surface-{lead:03}.npy")),
            npy_bytes("<f4", &[nc, 2], raw(&ground)),
        )?;
        let stats = json!({"lead":lead,"seconds":started.elapsed().as_secs_f64(),"grib_bytes":bytes.load(Ordering::Relaxed)-before,"chunks":nl*nf+2,"staged_value_bytes":(profiles.len()+ground.len())*4,"finite":true});
        atomic(&report, &stats)?;
        println!("{stats}");
    }
    Ok(meta)
}
fn write_stage(path: impl AsRef<Path>, bytes: Vec<u8>) -> Result<()> {
    let mut f = File::create(path)?;
    f.write_all(&bytes)?;
    // A durable checkpoint must never precede its staged data after power loss.
    f.sync_all()?;
    Ok(())
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
