//! NOAA NOMADS AIGFS index discovery and conditional, bounded GRIB range reads.
use crate::{
    ecmwf::{bounded, digest, mapping, DirectSource, Field, Hour, SurfaceObject, MAX_INDEX},
    state::*,
};
use anyhow::{bail, Context, Result};
use futures::{stream, StreamExt, TryStreamExt};
use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex, OnceLock},
    time::{Duration, Instant},
};

// NOMADS publishes a 120 requests/minute limit. One shared 750 ms gate leaves
// headroom and covers indexes, object metadata and payload ranges together.
static NEXT_REQUEST: OnceLock<Mutex<Instant>> = OnceLock::new();
fn request_clock() -> &'static Mutex<Instant> {
    NEXT_REQUEST.get_or_init(|| Mutex::new(Instant::now()))
}
async fn request_slot() {
    loop {
        let deadline = {
            let mut next = request_clock().lock().unwrap();
            let now = Instant::now();
            if *next <= now {
                *next = now + Duration::from_millis(750);
                return;
            }
            *next
        };
        tokio::time::sleep_until(deadline.into()).await;
    }
}
#[derive(Debug)]
pub struct RateLimited;
impl std::fmt::Display for RateLimited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "NOAA service unavailable or rate limited; retry after at least 60 seconds"
        )
    }
}
impl std::error::Error for RateLimited {}
fn cooldown() -> anyhow::Error {
    let mut next = request_clock().lock().unwrap();
    *next = (*next).max(Instant::now() + Duration::from_secs(60));
    RateLimited.into()
}
static READY_HOURS: OnceLock<Mutex<BTreeMap<(i64, u16), Hour>>> = OnceLock::new();
#[derive(Default)]
pub struct PayloadCache {
    groups: Mutex<BTreeMap<String, Arc<tokio::sync::OnceCell<Vec<u8>>>>>,
}
impl PayloadCache {
    pub async fn get(
        &self,
        client: &reqwest::Client,
        hour: &Hour,
        field: &Field,
    ) -> Result<Vec<u8>> {
        let mut fields = hour
            .fields
            .iter()
            .filter(|f| f.name == field.name)
            .collect::<Vec<_>>();
        fields.sort_by_key(|f| f.offset);
        let start = fields.first().context("Missing NOAA field group")?.offset;
        let mut end = start;
        for f in fields {
            anyhow::ensure!(f.offset == end, "Noncontiguous NOAA field group");
            end = end.checked_add(f.length).context("Group range overflow")?;
        }
        let length = end - start;
        anyhow::ensure!(length <= 128 * 1024 * 1024, "NOAA group exceeds bounds");
        let key = format!("{}:{}:{}", hour.init, hour.lead, field.name);
        let cell = self.groups.lock().unwrap().entry(key).or_default().clone();
        let bytes = cell
            .get_or_try_init(|| get_range(client, hour, field.pressure.is_none(), start, length))
            .await?;
        let offset = usize::try_from(field.offset - start)?;
        let last = offset
            .checked_add(usize::try_from(field.length)?)
            .context("Field range overflow")?;
        Ok(bytes
            .get(offset..last)
            .context("Truncated NOAA field group")?
            .to_vec())
    }
}

/// NetStorage ETags contain a content MD5 followed by replica mtime. NOAA can
/// return different mtimes for byte-identical replicas, even with If-Match.
/// Akamai documents comparing the MD5 component for ranged downloads:
/// https://techdocs.akamai.com/netstorage-usage/reference/usage-details-for-download
/// Accept only this exact known form; never discard arbitrary ETag suffixes.
pub fn content_identity(etag: &str) -> Result<&str> {
    let value = etag
        .strip_prefix('"')
        .and_then(|v| v.strip_suffix('"'))
        .context("Invalid NOAA ETag")?;
    let (md5, time) = value.split_once(':').context("Unknown NOAA ETag format")?;
    let (seconds, fraction) = time
        .split_once('.')
        .context("Unknown NOAA ETag timestamp")?;
    anyhow::ensure!(
        md5.len() == 32
            && md5.bytes().all(|b| b.is_ascii_hexdigit())
            && !seconds.is_empty()
            && seconds.len() <= 12
            && seconds.bytes().all(|b| b.is_ascii_digit())
            && !fraction.is_empty()
            && fraction.len() <= 9
            && fraction.bytes().all(|b| b.is_ascii_digit()),
        "Malformed NOAA content identity"
    );
    Ok(md5)
}

pub fn base_url(init: i64, lead: u16, surface: bool) -> Result<String> {
    anyhow::ensure!(
        init.rem_euclid(21600) == 0 && leads("aigfs").contains(&lead),
        "Invalid NOAA cycle/lead"
    );
    let date = chrono::DateTime::from_timestamp(init, 0).context("Invalid NOAA date")?;
    Ok(format!("https://nomads.ncep.noaa.gov/pub/data/nccf/com/aigfs/prod/aigfs.{}/{}/model/atmos/grib2/aigfs.t{}z.{}.f{lead:03}.grib2", date.format("%Y%m%d"), date.format("%H"), date.format("%H"), if surface {"sfc"} else {"pres"}))
}
fn parse_index(
    bytes: &[u8],
    init: i64,
    lead: u16,
    object_bytes: u64,
    surface: bool,
) -> Result<Vec<Field>> {
    let text = std::str::from_utf8(bytes)?;
    let lines = text
        .lines()
        .map(|line| line.split(':').collect::<Vec<_>>())
        .collect::<Vec<_>>();
    anyhow::ensure!(
        !lines.is_empty() && lines.len() <= 2048 && object_bytes <= 512 * 1024 * 1024,
        "Invalid NOAA index size"
    );
    let date = chrono::DateTime::from_timestamp(init, 0)
        .context("Invalid date")?
        .format("d=%Y%m%d%H")
        .to_string();
    let mut fields = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let mut previous_end = 0;
    for (i, parts) in lines.iter().enumerate() {
        anyhow::ensure!(
            parts.len() >= 6 && parts[0].parse::<usize>()? == i + 1 && parts[2] == date,
            "NOAA index identity differs"
        );
        let offset = parts[1].parse::<u64>()?;
        let end = if let Some(next) = lines.get(i + 1) {
            next.get(1).context("Missing offset")?.parse::<u64>()?
        } else {
            object_bytes
        };
        anyhow::ensure!(
            offset == previous_end && end > offset && end <= object_bytes,
            "Invalid NOAA ranges"
        );
        previous_end = end;
        let expected = if lead == 0 {
            "anl".to_owned()
        } else {
            format!("{lead} hour fcst")
        };
        // Accumulated precipitation is unrelated to the instantaneous retained fields.
        let param = match parts[3] {
            "HGT" => "gh",
            "TMP" => {
                if surface {
                    "2t"
                } else {
                    "t"
                }
            }
            "UGRD" => "u",
            "VGRD" => "v",
            "SPFH" => "q",
            _ => continue,
        };
        let pressure = if surface {
            if parts[3] != "TMP" || parts[4] != "2 m above ground" {
                continue;
            }
            None
        } else {
            let p = parts[4]
                .strip_suffix(" mb")
                .context("Invalid NOAA pressure")?
                .parse::<u16>()?;
            if !pressure_levels("aigfs").contains(&p) {
                continue;
            }
            Some(p)
        };
        anyhow::ensure!(
            parts[5] == expected && (20..=16 * 1024 * 1024).contains(&(end - offset)),
            "NOAA field forecast/range differs"
        );
        let name = mapping(param, pressure).context("Unexpected NOAA parameter")?;
        anyhow::ensure!(seen.insert((name, pressure)), "Duplicate NOAA field");
        fields.push(Field {
            name: name.to_owned(),
            pressure,
            param: param.to_owned(),
            offset,
            length: end - offset,
        });
    }
    if fields.len() != if surface { 1 } else { 40 } {
        return Err(crate::source::SourcePending("NOAA fields").into());
    }
    Ok(fields)
}
// Do not immediately retry NOAA's rate-limit 302/429 or server errors.
// A subsequent poll respects the shared cooldown, and no redirects are followed.
async fn metadata_request(
    client: &reqwest::Client,
    method: reqwest::Method,
    url: &str,
) -> Result<reqwest::Response> {
    request_slot().await;
    let response = client.request(method, url).send().await?;
    let status = response.status();
    if status.is_redirection()
        || status.is_server_error()
        || status == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(cooldown());
    }
    anyhow::ensure!(
        status == reqwest::StatusCode::OK || status == reqwest::StatusCode::NOT_FOUND,
        "NOAA metadata request failed for {url}: {status}"
    );
    Ok(response)
}

async fn object(
    client: &reqwest::Client,
    init: i64,
    lead: u16,
    surface: bool,
) -> Result<Option<(SurfaceObject, Vec<Field>)>> {
    let url = base_url(init, lead, surface)?;
    let response = metadata_request(client, reqwest::Method::GET, &format!("{url}.idx")).await?;
    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let bytes = bounded(response.error_for_status()?, MAX_INDEX).await?;
    let head = metadata_request(client, reqwest::Method::HEAD, &url).await?;
    if head.status() == reqwest::StatusCode::NOT_FOUND {
        return Ok(None);
    }
    let head = head.error_for_status()?;
    let etag = head
        .headers()
        .get("etag")
        .with_context(|| {
            format!(
                "Missing NOAA ETag for {url} (status {}, content-type {:?})",
                head.status(),
                head.headers().get("content-type")
            )
        })?
        .to_str()?
        .to_owned();
    let object_bytes = head
        .headers()
        .get("content-length")
        .context("Missing NOAA length")?
        .to_str()?
        .parse()?;
    content_identity(&etag)?;
    let fields = match parse_index(&bytes, init, lead, object_bytes, surface) {
        Ok(fields) => fields,
        Err(e) if e.downcast_ref::<crate::source::SourcePending>().is_some() => return Ok(None),
        Err(e) => return Err(e),
    };
    Ok(Some((
        SurfaceObject {
            etag,
            object_bytes,
            index_sha256: digest(&bytes),
        },
        fields,
    )))
}
pub(crate) async fn hour(client: &reqwest::Client, init: i64, lead: u16) -> Result<Option<Hour>> {
    let Some((pres, mut fields)) = object(client, init, lead, false).await? else {
        return Ok(None);
    };
    let Some((sfc, surface_fields)) = object(client, init, lead, true).await? else {
        return Ok(None);
    };
    fields.extend(surface_fields);
    Ok(Some(Hour {
        model: "aigfs".into(),
        surface_object: Some(sfc),
        init,
        lead,
        generation: String::new(),
        etag: pres.etag,
        object_bytes: pres.object_bytes,
        index_sha256: pres.index_sha256,
        fields,
    }))
}
pub async fn discover(mut times: Vec<i64>) -> Result<DirectSource> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(45))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let cache = READY_HOURS.get_or_init(|| Mutex::new(BTreeMap::new()));
    cache
        .lock()
        .unwrap()
        .retain(|(init, _), _| *init as f64 >= now() - 48. * 3600.);
    times.sort_unstable_by(|a, b| b.cmp(a));
    let mut hours = Vec::new();
    let mut complete_cycles = 0;
    for &init in &times {
        let mut found = stream::iter(leads("aigfs"))
            .map(|lead| {
                let client = &client;
                async move {
                    if let Some(hour) = cache.lock().unwrap().get(&(init, lead)).cloned() {
                        return Ok(Some(hour));
                    }
                    let found = hour(client, init, lead).await?;
                    if let Some(hour) = &found {
                        cache.lock().unwrap().insert((init, lead), hour.clone());
                    }
                    Ok::<_, anyhow::Error>(found)
                }
            })
            .buffer_unordered(4)
            .try_collect::<Vec<_>>()
            .await?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if found.len() == leads("aigfs").len() {
            complete_cycles += 1;
        }
        hours.append(&mut found);
        if complete_cycles >= 2 {
            break;
        }
    }
    hours.sort_by_key(|h| (h.init, h.lead));
    DirectSource::new("aigfs", hours, times)
}
async fn get_range(
    client: &reqwest::Client,
    hour: &Hour,
    surface: bool,
    offset: u64,
    length: u64,
) -> Result<Vec<u8>> {
    let (etag, total) = if surface {
        let object = hour
            .surface_object
            .as_ref()
            .context("Missing NOAA surface object")?;
        (&object.etag, object.object_bytes)
    } else {
        (&hour.etag, hour.object_bytes)
    };
    let end = offset.checked_add(length).context("NOAA range overflow")? - 1;
    request_slot().await;
    let response = client
        .get(base_url(hour.init, hour.lead, surface)?)
        .header("Range", format!("bytes={offset}-{end}"))
        .header("If-Match", etag)
        .send()
        .await?;
    if response.status().is_redirection()
        || response.status().is_server_error()
        || response.status() == reqwest::StatusCode::TOO_MANY_REQUESTS
    {
        return Err(cooldown());
    }
    if response.status() != reqwest::StatusCode::PARTIAL_CONTENT {
        bail!("NOAA conditional range failed: {}", response.status());
    }
    let actual_etag = response
        .headers()
        .get("etag")
        .context("Missing NOAA ETag")?
        .to_str()?;
    anyhow::ensure!(
        response
            .headers()
            .get("server")
            .and_then(|v| v.to_str().ok())
            == Some("AkamaiNetStorage")
            && content_identity(actual_etag)? == content_identity(etag)?
            && response
                .headers()
                .get("content-range")
                .and_then(|v| v.to_str().ok())
                == Some(format!("bytes {offset}-{end}/{total}").as_str()),
        "NOAA object content/range changed"
    );
    let bytes = bounded(response, usize::try_from(length)?).await?;
    anyhow::ensure!(bytes.len() as u64 == length, "Truncated NOAA range");
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn index() -> Vec<String> {
        let mut lines = Vec::new();
        for p in pressure_levels("aigfs") {
            for param in ["HGT", "TMP", "UGRD", "VGRD", "SPFH"] {
                lines.push(format!(
                    "{}:{}:d=2026102106:{param}:{p} mb:6 hour fcst:",
                    lines.len() + 1,
                    lines.len() * 100
                ));
            }
        }
        lines
    }
    #[tokio::test]
    async fn metadata_rate_limit_stops_requests_and_sets_a_minute_cooldown() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = calls.clone();
        let app = axum::Router::new().route(
            "/object",
            axum::routing::head(move || {
                let counter = counter.clone();
                async move {
                    counter.fetch_add(1, Ordering::SeqCst);
                    axum::http::StatusCode::FOUND
                }
            }),
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/object", listener.local_addr().unwrap());
        let server = tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        let client = reqwest::Client::builder()
            .redirect(reqwest::redirect::Policy::none())
            .no_proxy()
            .build()
            .unwrap();
        let error = metadata_request(&client, reqwest::Method::HEAD, &url)
            .await
            .unwrap_err();
        assert!(error.downcast_ref::<RateLimited>().is_some());
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(
            request_clock()
                .lock()
                .unwrap()
                .saturating_duration_since(Instant::now())
                > Duration::from_secs(59)
        );
        *request_clock().lock().unwrap() = Instant::now();
        server.abort();
    }
    #[test]
    fn replica_mtime_can_change_but_content_identity_cannot() {
        let first = "\"61737cca3e54b7e3a06e37d51d7baf58:1792557913.899031\"";
        let replica = "\"61737cca3e54b7e3a06e37d51d7baf58:1792557765.871091\"";
        assert_eq!(
            content_identity(first).unwrap(),
            content_identity(replica).unwrap()
        );
        assert_ne!(
            content_identity(first).unwrap(),
            content_identity(&replica.replace("61737", "01737")).unwrap()
        );
        for bad in [
            "W/\"61737cca3e54b7e3a06e37d51d7baf58:1.0\"",
            "\"checksum:1.0\"",
            "\"61737cca3e54b7e3a06e37d51d7baf58:unknown\"",
            "\"61737cca3e54b7e3a06e37d51d7baf58:1.0:extra\"",
        ] {
            assert!(content_identity(bad).is_err());
        }
    }
    #[test]
    fn index_binds_every_required_field_to_cycle_lead_and_range() {
        let init = timestamp("2026-10-21T06:00:00").unwrap() as i64;
        let lines = index();
        assert_eq!(
            parse_index(lines.join("\n").as_bytes(), init, 6, 4000, false)
                .unwrap()
                .len(),
            40
        );
        for (from, to) in [
            ("d=2026102106", "d=2026102100"),
            ("6 hour fcst", "12 hour fcst"),
            ("2:100:", "2:0:"),
            ("TMP:1000 mb", "HGT:1000 mb"),
        ] {
            assert!(parse_index(
                lines.join("\n").replace(from, to).as_bytes(),
                init,
                6,
                4000,
                false
            )
            .is_err());
        }
        assert!(parse_index(lines[..39].join("\n").as_bytes(), init, 6, 3900, false).is_err());
        assert!(parse_index(lines.join("\n").as_bytes(), init, 6, 3999, false).is_ok());
        assert!(parse_index(lines.join("\n").as_bytes(), init, 6, 3901, false).is_err());
        let sfc = b"1:0:d=2026102106:TMP:2 m above ground:6 hour fcst:";
        let fields = parse_index(sfc, init, 6, 100, true).unwrap();
        assert_eq!(fields.len(), 1);
        assert_eq!(fields[0].name, "temperature_2m");
        assert!(fields[0].pressure.is_none());
    }
}
