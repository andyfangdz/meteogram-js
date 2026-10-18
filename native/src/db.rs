use crate::state::{fields, ident};
use anyhow::{bail, Context, Result};
use serde_json::{json, Value};
use std::{
    io::{Cursor, Read},
    time::{Duration, Instant},
};
#[derive(Clone)]
pub struct Db {
    http: reqwest::blocking::Client,
    pub url: String,
}
impl Db {
    pub fn new() -> Result<Self> {
        let url = std::env::var("METEOGRAM_CLICKHOUSE_URL")
            .unwrap_or_else(|_| "http://127.0.0.1:18123".into());
        let u = url::Url::parse(&url)?;
        if !matches!(u.host_str(), Some("127.0.0.1" | "localhost" | "::1")) {
            bail!("ClickHouse URL must be loopback");
        }
        Ok(Self {
            http: reqwest::blocking::Client::builder()
                .timeout(Duration::from_secs(120))
                .no_proxy()
                .build()?,
            url,
        })
    }
    pub fn bytes(&self, query: &str) -> Result<Vec<u8>> {
        let r = self
            .http
            .post(&self.url)
            .query(&[("max_query_size", "2097152")])
            .body(query.to_owned())
            .send()?;
        let status = r.status();
        let b = r.bytes()?.to_vec();
        if !status.is_success() {
            bail!(
                "ClickHouse {status}: {}",
                String::from_utf8_lossy(&b)
                    .chars()
                    .take(2000)
                    .collect::<String>()
            );
        }
        Ok(b)
    }
    pub fn command(&self, q: &str) -> Result<()> {
        self.bytes(q)?;
        Ok(())
    }
    pub fn json(&self, q: &str) -> Result<Vec<Value>> {
        let b = self.bytes(&format!("{q} FORMAT JSONEachRow"))?;
        std::str::from_utf8(&b)?
            .lines()
            .filter(|l| !l.is_empty())
            .map(|s| Ok(serde_json::from_str(s)?))
            .collect()
    }
    pub fn scalar(&self, q: &str) -> Result<u64> {
        let b = self.bytes(&format!("{q} FORMAT TabSeparated"))?;
        Ok(std::str::from_utf8(&b)?.trim().parse()?)
    }
    pub fn insert(&self, table: &str, body: Vec<u8>) -> Result<()> {
        ident(table)?;
        let r = self
            .http
            .post(&self.url)
            .query(&[
                (
                    "query",
                    format!("INSERT INTO meteogram_native.{table} FORMAT Native"),
                ),
                ("max_threads", "2".into()),
                ("wait_end_of_query", "1".into()),
            ])
            .body(body)
            .send()?;
        let status = r.status();
        let bytes = r.bytes()?;
        if !status.is_success() || !bytes.iter().all(u8::is_ascii_whitespace) {
            bail!(
                "Insert {status}: {}",
                String::from_utf8_lossy(&bytes)
                    .chars()
                    .take(2000)
                    .collect::<String>()
            );
        }
        Ok(())
    }
    pub fn profile(
        &self,
        database: &str,
        table: &str,
        m: &Value,
        cell: u32,
    ) -> Result<(Vec<Value>, f64)> {
        self.profile_with_dictionary(database, table, m, cell, None)
    }
    pub fn profile_with_dictionary(
        &self,
        database: &str,
        table: &str,
        m: &Value,
        cell: u32,
        dictionary: Option<&crate::dictionary::Dictionary>,
    ) -> Result<(Vec<Value>, f64)> {
        anyhow::ensure!(
            !crate::dictionary::encoded(m) || dictionary.is_some(),
            "Missing profile dictionary"
        );
        ident(database)?;
        ident(table)?;
        let fs = fields(m);
        for f in &fs {
            ident(f)?;
        }
        let tiled = crate::packed::tiled(m);
        let tile_cells = if tiled {
            crate::packed::tile_cells(m)?
        } else {
            1
        };
        let point = if tiled {
            Some(
                dictionary
                    .context("Missing packed dictionary")?
                    .point_index(cell)?,
            )
        } else {
            None
        };
        let profile = m["storage"]["layout"] == "cell_profile_v1" || tiled;
        let cols = fs.join(",");
        let q = if let Some(point) = point {
            format!("SELECT cell_ids,{cols} FROM {database}.{table} PREWHERE tile_id={} LIMIT 2 FORMAT RowBinary",point/tile_cells)
        } else if profile {
            format!("SELECT {cols} FROM {database}.{table} PREWHERE cell_id={cell} LIMIT 2 FORMAT RowBinary")
        } else {
            format!("SELECT lead_hour,{cols} FROM {database}.{table} PREWHERE cell_id={cell} ORDER BY lead_hour FORMAT RowBinary")
        };
        let start = Instant::now();
        let bytes = self.bytes(&q)?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let mut c = Cursor::new(bytes.as_slice());
        let nf = m["variables"].as_array().context("Missing fields")?.len();
        let nl = m["levels"].as_array().context("Missing levels")?.len();
        let leads = m["leads"].as_array().context("Missing leads")?;
        let nt = leads.len();
        let bit = m["storage"]["encoding"] == "float32_bits";
        let mut rows = vec![];
        if profile {
            let lane = if let Some(point) = point {
                let lane = point % tile_cells;
                let ids = array(&mut c, 4)?;
                anyhow::ensure!(
                    ids.len() <= tile_cells && ids.get(lane) == Some(&cell),
                    "Incorrect packed cell association"
                );
                lane
            } else {
                0
            };
            let mut arrays = vec![];
            for _ in &fs {
                arrays.push(if tiled {
                    packed_array(&mut c, lane, tile_cells / crate::packed::LANES)?
                } else {
                    array(&mut c, crate::dictionary::index_bytes(m)?)?
                });
            }
            if c.position() != bytes.len() as u64 {
                bail!("Duplicate cell profile");
            }
            for (t, lead) in leads.iter().enumerate() {
                let mut row = vec![lead.clone()];
                for (f, a) in arrays.iter().enumerate() {
                    let width = if f < nf { nl * nt } else { nt };
                    if a.len() != width {
                        bail!("Malformed profile length");
                    }
                    let value = |i: usize| -> Result<Value> {
                        let bits = if let Some(dictionary) = dictionary {
                            dictionary.value(f, i, a[i])?
                        } else {
                            a[i]
                        };
                        let v = if bit {
                            f32::from_bits(bits)
                        } else {
                            f32::from_le_bytes(bits.to_le_bytes())
                        };
                        if v.is_finite() {
                            Ok(json!(v as f64))
                        } else {
                            Ok(Value::Null)
                        }
                    };
                    if f < nf {
                        let vals = (0..nl)
                            .map(|l| {
                                value(if m["storage"]["axes"] == "level_lead" {
                                    l * nt + t
                                } else {
                                    t * nl + l
                                })
                            })
                            .collect::<Result<Vec<_>>>()?;
                        row.push(Value::Array(vals));
                    } else {
                        row.push(value(t)?);
                    }
                }
                rows.push(Value::Array(row));
            }
        } else {
            while c.position() < bytes.len() as u64 {
                let mut b = [0; 2];
                c.read_exact(&mut b)?;
                let mut row = vec![json!(u16::from_le_bytes(b))];
                for f in 0..fs.len() {
                    if f < nf {
                        row.push(Value::Array(
                            array(&mut c, 4)?
                                .into_iter()
                                .map(|bits| json!(f32::from_bits(bits) as f64))
                                .collect(),
                        ));
                    } else {
                        let mut b = [0; 4];
                        c.read_exact(&mut b)?;
                        row.push(json!(f32::from_le_bytes(b) as f64));
                    }
                }
                rows.push(Value::Array(row));
            }
        }
        if rows.len() != nt || rows.iter().zip(leads).any(|(r, l)| r[0] != *l) {
            bail!("Incomplete profile");
        }
        Ok((rows, elapsed))
    }
}
pub fn varuint(c: &mut Cursor<&[u8]>) -> Result<usize> {
    let mut x = 0usize;
    for shift in (0..64).step_by(7) {
        let mut b = [0];
        c.read_exact(&mut b)?;
        x |= ((b[0] & 127) as usize) << shift;
        if b[0] < 128 {
            return Ok(x);
        }
    }
    bail!("Invalid varuint")
}
fn array(c: &mut Cursor<&[u8]>, bytes: usize) -> Result<Vec<u32>> {
    let n = varuint(c)?;
    if n > 1000000 {
        bail!("Oversized profile");
    }
    let mut a = Vec::with_capacity(n);
    for _ in 0..n {
        let mut b = [0; 4];
        c.read_exact(&mut b[..bytes])?;
        a.push(u32::from_le_bytes(b));
    }
    Ok(a)
}
fn packed_array(c: &mut Cursor<&[u8]>, lane: usize, groups: usize) -> Result<Vec<u32>> {
    let n = varuint(c)?;
    anyhow::ensure!(
        n <= 1_000_000
            && groups > 0
            && n.is_multiple_of(groups)
            && lane < groups * crate::packed::LANES,
        "Oversized packed profile"
    );
    let start = c.position() as usize;
    let end = start
        .checked_add(n * 16)
        .context("Packed profile overflow")?;
    anyhow::ensure!(end <= c.get_ref().len(), "Truncated packed profile");
    let mut values = Vec::with_capacity(n / groups);
    for slot in 0..n / groups {
        let at = start + (slot * groups + lane / crate::packed::LANES) * 16;
        let bytes: &[u8; 16] = c.get_ref()[at..at + 16].try_into()?;
        values.push(crate::packed::code(
            u128::from_le_bytes(*bytes),
            lane % crate::packed::LANES,
        ));
    }
    c.set_position(end as u64);
    Ok(values)
}
