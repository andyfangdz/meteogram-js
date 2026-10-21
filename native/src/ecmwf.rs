//! Version-pinned ECMWF Open Data acquisition for the existing AIFS product.
//! Only discovery and encoded GRIB reads differ from the Icechunk source path.
use crate::{source::SourcePending, state::*};
use anyhow::{bail, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, sync::Mutex, time::Duration};

pub const PROVIDER: &str = "ecmwf_open_data_v1";
const BASE: &str = "https://storage.googleapis.com/ecmwf-open-data";
const MAX_INDEX: usize = 2 * 1024 * 1024;
const MAX_MESSAGE: u64 = 16 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Field {
    pub name: String,
    pub pressure: Option<u16>,
    pub param: String,
    pub offset: u64,
    pub length: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Hour {
    pub init: i64,
    pub lead: u16,
    pub generation: String,
    pub etag: String,
    pub object_bytes: u64,
    pub index_sha256: String,
    pub fields: Vec<Field>,
}
pub struct DirectSource {
    pub hours: Vec<Hour>,
    pub times: Vec<i64>,
    client: reqwest::Client,
    proofs: Mutex<BTreeMap<String, String>>,
}

fn client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()?)
}
pub fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn base_url(init: i64, lead: u16) -> Result<String> {
    let date = chrono::DateTime::from_timestamp(init, 0).context("Invalid ECMWF initialization")?;
    anyhow::ensure!(
        init.rem_euclid(21600) == 0 && leads("aifs").contains(&lead),
        "Invalid ECMWF cycle/lead"
    );
    Ok(format!(
        "{BASE}/{}/{}z/aifs-single/0p25/oper/{}-{lead}h-oper-fc",
        date.format("%Y%m%d"),
        date.format("%H"),
        date.format("%Y%m%d%H0000")
    ))
}
async fn bounded(mut response: reqwest::Response, limit: usize) -> Result<Vec<u8>> {
    anyhow::ensure!(
        response.content_length().is_none_or(|n| n <= limit as u64),
        "Oversized ECMWF response"
    );
    let mut bytes = Vec::new();
    while let Some(chunk) = response.chunk().await? {
        anyhow::ensure!(
            bytes
                .len()
                .checked_add(chunk.len())
                .is_some_and(|n| n <= limit),
            "Oversized ECMWF response"
        );
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}
fn mapping(param: &str, pressure: Option<u16>) -> Option<&'static str> {
    match (param, pressure.is_some()) {
        ("z", true) => Some("pressure_level/geopotential_height"),
        ("t", true) => Some("pressure_level/temperature"),
        ("u", true) => Some("pressure_level/wind_u"),
        ("v", true) => Some("pressure_level/wind_v"),
        ("q", true) => Some("pressure_level/specific_humidity"),
        ("2t", false) => Some("temperature_2m"),
        ("2d", false) => Some("dew_point_temperature_2m"),
        _ => None,
    }
}
fn parse_index(bytes: &[u8], init: i64, lead: u16, object_bytes: u64) -> Result<Vec<Field>> {
    anyhow::ensure!(
        bytes.len() <= MAX_INDEX && object_bytes <= 512 * 1024 * 1024,
        "ECMWF source exceeds bounds"
    );
    let date = chrono::DateTime::from_timestamp(init, 0).context("Invalid init")?;
    let mut fields = BTreeMap::new();
    let mut previous_end = 0;
    for (count, line) in bytes
        .split(|b| *b == b'\n')
        .filter(|l| !l.is_empty())
        .enumerate()
    {
        anyhow::ensure!(count < 2048, "Too many ECMWF messages");
        let value: Value = serde_json::from_slice(line)?;
        anyhow::ensure!(
            value["date"] == date.format("%Y%m%d").to_string()
                && value["time"] == date.format("%H00").to_string()
                && value["step"].as_str().and_then(|v| v.parse::<u16>().ok()) == Some(lead)
                && value["model"] == "aifs-single"
                && value["type"] == "fc"
                && value["stream"] == "oper"
                && value["class"] == "ai",
            "ECMWF index forecast identity differs"
        );
        let offset = value["_offset"].as_u64().context("Missing GRIB offset")?;
        let length = value["_length"].as_u64().context("Missing GRIB length")?;
        let end = offset.checked_add(length).context("GRIB range overflow")?;
        anyhow::ensure!(
            (20..=MAX_MESSAGE).contains(&length) && offset >= previous_end && end <= object_bytes,
            "Invalid ECMWF message ranges"
        );
        previous_end = end;
        let pressure = match value["levtype"].as_str() {
            Some("pl") => {
                let p: u16 = value["levelist"]
                    .as_str()
                    .context("Missing pressure")?
                    .parse()?;
                if !pressure_levels("aifs").contains(&p) {
                    continue;
                }
                Some(p)
            }
            Some("sfc") => None,
            _ => continue,
        };
        let param = value["param"].as_str().context("Missing parameter")?;
        if let Some(name) = mapping(param, pressure) {
            let field = Field {
                name: name.to_owned(),
                pressure,
                param: param.to_owned(),
                offset,
                length,
            };
            anyhow::ensure!(
                fields.insert((name.to_owned(), pressure), field).is_none(),
                "Duplicate ECMWF field"
            );
        }
    }
    if fields.len() != 42 {
        return Err(SourcePending("ECMWF fields").into());
    }
    Ok(fields.into_values().collect())
}
async fn discover_hour(client: &reqwest::Client, init: i64, lead: u16) -> Result<Option<Hour>> {
    let base = base_url(init, lead)?;
    let response = client.get(format!("{base}.index")).send().await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = bounded(response.error_for_status()?, MAX_INDEX).await?;
    let head = client.head(format!("{base}.grib2")).send().await?;
    if head.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let head = head.error_for_status()?;
    let header = |name| -> Result<String> {
        Ok(head
            .headers()
            .get(name)
            .context("Missing ECMWF object identity")?
            .to_str()?
            .to_owned())
    };
    let object_bytes: u64 = header("content-length")?.parse()?;
    let fields = match parse_index(&bytes, init, lead, object_bytes) {
        Ok(fields) => fields,
        Err(e) if e.downcast_ref::<SourcePending>().is_some() => return Ok(None),
        Err(e) => return Err(e),
    };
    let hour = Hour {
        init,
        lead,
        generation: header("x-goog-generation")?,
        etag: header("etag")?,
        object_bytes,
        index_sha256: digest(&bytes),
        fields,
    };
    validate_hour(&hour)?;
    Ok(Some(hour))
}
fn validate_hour(hour: &Hour) -> Result<()> {
    base_url(hour.init, hour.lead)?;
    anyhow::ensure!(
        !hour.generation.is_empty()
            && hour.generation.bytes().all(|b| b.is_ascii_digit())
            && hour.etag.starts_with('"')
            && hour.etag.ends_with('"')
            && hour.etag.len() <= 128
            && hour.index_sha256.len() == 64
            && hour.index_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            && hour.object_bytes <= 512 * 1024 * 1024
            && hour.fields.len() == 42,
        "Invalid pinned ECMWF source"
    );
    let mut seen = std::collections::BTreeSet::new();
    for field in &hour.fields {
        anyhow::ensure!(
            mapping(&field.param, field.pressure) == Some(field.name.as_str())
                && field
                    .pressure
                    .is_none_or(|p| pressure_levels("aifs").contains(&p))
                && seen.insert((&field.name, field.pressure))
                && (20..=MAX_MESSAGE).contains(&field.length)
                && field
                    .offset
                    .checked_add(field.length)
                    .is_some_and(|end| end <= hour.object_bytes),
            "Invalid pinned ECMWF field"
        );
    }
    Ok(())
}
impl DirectSource {
    /// Candidate cycle times are discovery probes, not evidence of published runs.
    pub fn available_times(&self) -> Vec<i64> {
        self.times
            .iter()
            .copied()
            .filter(|t| self.hours.iter().any(|h| h.init == *t))
            .collect()
    }
    pub fn new(hours: Vec<Hour>, mut times: Vec<i64>) -> Result<Self> {
        for hour in &hours {
            validate_hour(hour)?;
        }
        times.sort_unstable();
        times.dedup();
        anyhow::ensure!(
            hours.iter().all(|h| times.contains(&h.init)),
            "ECMWF time axis differs"
        );
        Ok(Self {
            hours,
            times,
            client: client()?,
            proofs: Mutex::new(BTreeMap::new()),
        })
    }
    pub fn from_pin(pin: &Value) -> Result<Self> {
        anyhow::ensure!(
            pin["source_provider"] == PROVIDER && pin["model"] == "aifs",
            "Invalid ECMWF pin provider"
        );
        let hour: Hour = serde_json::from_value(pin["ecmwf_source"].clone())?;
        anyhow::ensure!(
            pin["snapshot_id"] == Self::hour_snapshot(&hour)?
                && timestamp(pin["init_time"].as_str().context("Missing init")?)?
                    == hour.init as f64,
            "ECMWF pin identity differs"
        );
        Self::new(vec![hour.clone()], vec![hour.init])
    }
    pub fn hour_snapshot(hour: &Hour) -> Result<String> {
        Ok(format!("ecmwf-{}", digest(&serde_json::to_vec(hour)?)))
    }
    pub fn snapshot(&self) -> Result<String> {
        Ok(format!(
            "ecmwf-{}",
            digest(&serde_json::to_vec(&self.hours)?)
        ))
    }
    pub fn pin(&self, pin: &Value, lead: u16) -> Result<Value> {
        let init = timestamp(pin["init_time"].as_str().context("Missing init")?)? as i64;
        let hour = self
            .hours
            .iter()
            .find(|h| h.init == init && h.lead == lead)
            .context("ECMWF hour not ready")?;
        let mut pin = pin.clone();
        pin["source_provider"] = json!(PROVIDER);
        pin["ecmwf_source"] = serde_json::to_value(hour)?;
        pin["snapshot_id"] = json!(Self::hour_snapshot(hour)?);
        Ok(pin)
    }
    pub fn coord(&self, name: &str) -> Result<Vec<f64>> {
        Ok(match name {
            "init_time" => self.times.iter().map(|t| *t as f64).collect(),
            "lead_time" => leads("aifs")
                .iter()
                .map(|h| f64::from(*h) * 3600.)
                .collect(),
            "pressure_level/pressure_level" => pressure_levels("aifs")
                .iter()
                .map(|p| f64::from(*p))
                .collect(),
            "latitude" => (0..721).map(|i| 90. - f64::from(i) * 0.25).collect(),
            "longitude" => (0..1440).map(|i| -180. + f64::from(i) * 0.25).collect(),
            _ => bail!("Unknown ECMWF coordinate {name}"),
        })
    }
    pub fn meta(&self, name: &str) -> Result<Value> {
        if let Ok(coord) = self.coord(name) {
            let units = match name {
                "init_time" => "seconds since 1970-01-01",
                "lead_time" => "seconds",
                _ => "",
            };
            return Ok(json!({"attributes":{"units":units},"shape":[coord.len()]}));
        }
        let (param, units, temperature) = match name {
            "pressure_level/geopotential_height" => ("z", "m", false),
            "pressure_level/temperature" | "temperature_2m" => ("t", "degree_Celsius", true),
            "dew_point_temperature_2m" => ("d", "degree_Celsius", true),
            "pressure_level/wind_u" => ("u", "m s-1", false),
            "pressure_level/wind_v" => ("v", "m s-1", false),
            "pressure_level/specific_humidity" => ("q", "1", false),
            _ => bail!("Unknown ECMWF field {name}"),
        };
        let mut dims = vec!["init_time", "lead_time", "latitude", "longitude"];
        let mut shape = vec![self.times.len(), 31, 721, 1440];
        let mut chunks = vec![1, 1, 721, 1440];
        if name.starts_with("pressure_level/") {
            dims.push("pressure_level");
            shape.push(8);
            chunks.push(1);
        }
        let mut codecs = Vec::new();
        if param == "z" {
            codecs.push(json!({"name":"scale_offset","configuration":{"scale":9.80665}}));
        }
        if temperature {
            codecs.push(json!({"name":"scale_offset","configuration":{"offset":-273.15}}));
        }
        codecs.push(json!({"name":"gribberish","configuration":{"var":param,"adjust_longitude_range":true,"north_up":true}}));
        Ok(
            json!({"data_type":"float64","dimension_names":dims,"shape":shape,"chunk_grid":{"configuration":{"chunk_shape":chunks}},"attributes":{"units":units},"codecs":codecs}),
        )
    }
    fn reference(&self, key: &str) -> Result<(&Hour, &Field)> {
        let (name, coords) = key.split_once("/c/").context("Invalid ECMWF key")?;
        let coords: Vec<usize> = coords
            .split('/')
            .map(str::parse)
            .collect::<std::result::Result<_, _>>()?;
        let pressure = name.starts_with("pressure_level/");
        anyhow::ensure!(
            coords.len() == if pressure { 5 } else { 4 } && coords[2] == 0 && coords[3] == 0,
            "Invalid ECMWF chunk coordinates"
        );
        let init = self
            .times
            .get(coords[0])
            .context("Invalid ECMWF time index")?;
        let lead = *leads("aifs")
            .get(coords[1])
            .context("Invalid ECMWF lead index")?;
        let p = if pressure {
            Some(
                *pressure_levels("aifs")
                    .get(coords[4])
                    .context("Invalid ECMWF level index")?,
            )
        } else {
            None
        };
        let hour = self
            .hours
            .iter()
            .find(|h| h.init == *init && h.lead == lead)
            .context("ECMWF source hour missing")?;
        let field = hour
            .fields
            .iter()
            .find(|f| f.name == name && f.pressure == p)
            .context("ECMWF field missing")?;
        Ok((hour, field))
    }
    pub fn exists(&self, key: &str) -> bool {
        self.reference(key).is_ok()
    }
    pub async fn get(&self, key: &str) -> Result<Vec<u8>> {
        let (hour, field) = self.reference(key)?;
        let end = field
            .offset
            .checked_add(field.length)
            .context("Range overflow")?
            - 1;
        let response = self
            .client
            .get(format!(
                "{}.grib2?generation={}",
                base_url(hour.init, hour.lead)?,
                hour.generation
            ))
            .header("Range", format!("bytes={}-{end}", field.offset))
            .header("If-Match", &hour.etag)
            .send()
            .await?;
        anyhow::ensure!(
            response.status() == reqwest::StatusCode::PARTIAL_CONTENT,
            "ECMWF range request failed: {}",
            response.status()
        );
        let headers = response.headers();
        anyhow::ensure!(
            headers.get("content-range").and_then(|v| v.to_str().ok())
                == Some(format!("bytes {}-{end}/{}", field.offset, hour.object_bytes).as_str())
                && headers.get("etag").and_then(|v| v.to_str().ok()) == Some(hour.etag.as_str())
                && headers
                    .get("x-goog-generation")
                    .and_then(|v| v.to_str().ok())
                    == Some(hour.generation.as_str()),
            "ECMWF object/range identity changed"
        );
        let bytes = bounded(response, field.length as usize).await?;
        self.validate(key, &bytes)?;
        Ok(bytes)
    }
    pub fn validate(&self, key: &str, bytes: &[u8]) -> Result<()> {
        let (hour, field) = self.reference(key)?;
        validate_message(hour, field, bytes)?;
        let hash = digest(bytes);
        let mut proofs = self.proofs.lock().unwrap();
        if let Some(previous) = proofs.insert(key.to_owned(), hash.clone()) {
            anyhow::ensure!(previous == hash, "ECMWF message changed within pinned hour");
        }
        Ok(())
    }
    pub fn proofs(&self) -> Value {
        json!(*self.proofs.lock().unwrap())
    }
}

fn validate_message(hour: &Hour, field: &Field, bytes: &[u8]) -> Result<()> {
    use gribberish::templates::product::tables::FixedSurfaceType;
    anyhow::ensure!(
        bytes.len() as u64 == field.length
            && bytes.starts_with(b"GRIB")
            && bytes.ends_with(b"7777")
            && bytes[7] == 2,
        "Truncated/invalid ECMWF GRIB"
    );
    let msg = gribberish::message::Message::from_data(bytes, 0).context("Invalid ECMWF GRIB")?;
    let (category, parameter) = match field.param.as_str() {
        "z" => (3, 4),
        "t" | "2t" => (0, 0),
        "2d" => (0, 6),
        "u" => (2, 2),
        "v" => (2, 3),
        "q" => (1, 0),
        _ => bail!("Unexpected field"),
    };
    anyhow::ensure!(
        msg.len() == bytes.len()
            && msg.discipline_value()? == 0
            && msg.category_value()? == category
            && msg.parameter_value()? == parameter
            && msg.reference_date()?.timestamp() == hour.init
            && msg.forecast_date()?.timestamp() == hour.init + i64::from(hour.lead) * 3600
            && msg.grid_template_id()? == 0
            && msg.data_point_count()? == 721 * 1440,
        "ECMWF GRIB identity/grid differs"
    );
    let (kind, level) = msg.first_fixed_surface()?;
    anyhow::ensure!(
        if let Some(p) = field.pressure {
            kind == FixedSurfaceType::IsobaricSurface && level == Some(f64::from(p) * 100.)
        } else {
            kind == FixedSurfaceType::SpecifiedHeightLevelAboveGround && level == Some(2.)
        },
        "ECMWF GRIB vertical coordinate differs"
    );
    let projection = msg.latlng_projector()?;
    let (lat, lon) = projection.lat_lng_adjusted(true, true);
    anyhow::ensure!(
        lat.len() == 721
            && lon.len() == 1440
            && lat
                .iter()
                .enumerate()
                .all(|(i, x)| *x == 90. - i as f64 * 0.25)
            && lon
                .iter()
                .enumerate()
                .all(|(i, x)| *x == -180. + i as f64 * 0.25),
        "ECMWF coordinates differ"
    );
    Ok(())
}

pub async fn discover(times: Vec<i64>) -> Result<DirectSource> {
    let client = client()?;
    let requests = times
        .iter()
        .flat_map(|t| leads("aifs").into_iter().map(move |h| (*t, h)))
        .collect::<Vec<_>>();
    let hours = stream::iter(requests)
        .map(|(t, h)| {
            let client = client.clone();
            async move { discover_hour(&client, t, h).await }
        })
        .buffer_unordered(4)
        .try_collect::<Vec<_>>()
        .await?
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let mut hours = hours;
    hours.sort_by_key(|h| (h.init, h.lead));
    DirectSource::new(hours, times)
}

pub fn check_metadata(meta: &Value) -> Result<()> {
    if meta["source_provider"] != PROVIDER {
        return Ok(());
    }
    // Parent manifests describe multiple child hours; each child is independently bound.
    if partitioned(meta) {
        return Ok(());
    }
    let direct = DirectSource::from_pin(meta)?;
    anyhow::ensure!(
        meta["leads"] == json!([direct.hours[0].lead]),
        "ECMWF staged lead differs"
    );
    if meta["staging"]["complete"] == true || meta.get("storage").is_some() {
        let proofs = meta["source_payload_sha256"]
            .as_object()
            .context("Missing ECMWF payload hashes")?;
        anyhow::ensure!(proofs.len() == 42, "Incomplete ECMWF payload proof");
        for (key, value) in proofs {
            anyhow::ensure!(
                direct.exists(key)
                    && value
                        .as_str()
                        .is_some_and(|v| v.len() == 64 && v.bytes().all(|b| b.is_ascii_hexdigit())),
                "Invalid ECMWF payload proof"
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index() -> Vec<Value> {
        let mut entries = Vec::new();
        for p in pressure_levels("aifs") {
            for param in ["z", "t", "u", "v", "q"] {
                entries.push(json!({"date":"20261021","time":"0600","step":"0","model":"aifs-single","type":"fc","stream":"oper","class":"ai","levtype":"pl","levelist":p.to_string(),"param":param,"_offset":entries.len()*100,"_length":100}));
            }
        }
        for param in ["2t", "2d"] {
            entries.push(json!({"date":"20261021","time":"0600","step":"0","model":"aifs-single","type":"fc","stream":"oper","class":"ai","levtype":"sfc","param":param,"_offset":entries.len()*100,"_length":100}));
        }
        entries
    }
    fn parse(entries: &[Value]) -> Result<Vec<Field>> {
        let bytes = entries
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        parse_index(
            bytes.as_bytes(),
            timestamp("2026-10-21T06:00:00").unwrap() as i64,
            0,
            100000,
        )
    }
    #[test]
    fn direct_index_requires_exact_fields_and_identity() {
        let mut entries = index();
        assert_eq!(parse(&entries).unwrap().len(), 42);
        entries.pop();
        assert!(parse(&entries)
            .unwrap_err()
            .downcast_ref::<SourcePending>()
            .is_some());
        let mut entries = index();
        entries[0]["step"] = json!(6);
        assert!(parse(&entries).is_err());
        let mut entries = index();
        entries[0]["model"] = json!("ifs");
        assert!(parse(&entries).is_err());
        let mut entries = index();
        entries[1]["param"] = json!("z");
        assert!(parse(&entries).is_err());
    }
    #[test]
    fn direct_index_rejects_overlap_overflow_and_oversize() {
        for (key, value) in [
            ("_offset", json!(u64::MAX)),
            ("_length", json!(MAX_MESSAGE + 1)),
            ("_offset", json!(0)),
        ] {
            let mut entries = index();
            entries[1][key] = value;
            assert!(parse(&entries).is_err());
        }
    }
    #[test]
    fn direct_pin_is_bound_to_object_and_hour() {
        let hour = Hour {
            init: timestamp("2026-10-21T06:00:00").unwrap() as i64,
            lead: 0,
            generation: "123".into(),
            etag: "\"abc\"".into(),
            object_bytes: 100000,
            index_sha256: "a".repeat(64),
            fields: parse(&index()).unwrap(),
        };
        let source =
            DirectSource::new(vec![hour.clone()], vec![hour.init, hour.init + 21600]).unwrap();
        assert_eq!(source.available_times(), vec![hour.init]);
        let pin = source
            .pin(
                &json!({"model":"aifs","init_time":"2026-10-21T06:00:00"}),
                0,
            )
            .unwrap();
        assert!(DirectSource::from_pin(&pin).is_ok());
        let mut changed = pin.clone();
        changed["ecmwf_source"]["generation"] = json!("124");
        assert!(DirectSource::from_pin(&changed).is_err());
        assert!(source.pin(&pin, 6).is_err());
        assert!(source
            .validate("pressure_level/temperature/c/0/0/0/0/0", b"broken")
            .is_err());
    }
}
