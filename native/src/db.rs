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
    pub fn profile_leads<D: std::ops::Deref<Target = crate::dictionary::Dictionary>>(
        &self,
        database: &str,
        table: &str,
        m: &Value,
        cell: u32,
        point: usize,
        books: &std::collections::BTreeMap<u16, D>,
    ) -> Result<(Vec<Value>, f64)> {
        ident(database)?;
        ident(table)?;
        let fields = fields(m);
        for name in &fields {
            ident(name)?;
        }
        let nf = m["variables"].as_array().context("Fields")?.len();
        let nl = m["levels"].as_array().context("Levels")?.len();
        let leads: Vec<u16> = serde_json::from_value(m["leads"].clone())?;
        anyhow::ensure!(
            !leads.is_empty() && leads.iter().copied().eq(books.keys().copied()),
            "Incomplete run dictionaries"
        );
        let group = (point % crate::packed::CELLS) / crate::packed::LANES;
        let cols=fields.iter().map(|name|format!("length({name}),arrayMap(i -> {name}[i],range(toUInt64({}),length({name})+toUInt64(1),toUInt64({})))",group+1,crate::packed::GROUPS)).collect::<Vec<_>>().join(",");
        let hours = leads
            .iter()
            .map(u16::to_string)
            .collect::<Vec<_>>()
            .join(",");
        // The catalog's immutable hour set excludes every unfinished partition.
        let query=format!("SELECT lead_hour,cell_ids,{cols} FROM {database}.{table} PREWHERE tile_id={} AND lead_hour IN ({hours}) ORDER BY lead_hour SETTINGS max_threads=2,max_block_size=1 FORMAT RowBinary",point/crate::packed::CELLS);
        let start = Instant::now();
        let bytes = self.bytes(&query)?;
        let elapsed = start.elapsed().as_secs_f64() * 1000.;
        let mut cursor = Cursor::new(bytes.as_slice());
        let mut rows = Vec::new();
        for lead in leads {
            let mut b = [0; 2];
            cursor.read_exact(&mut b)?;
            anyhow::ensure!(
                u16::from_le_bytes(b) == lead,
                "Duplicate/missing forecast hour"
            );
            let ids = array(&mut cursor, 4)?;
            anyhow::ensure!(
                ids.get(point % crate::packed::CELLS) == Some(&cell)
                    && ids.len() <= crate::packed::CELLS,
                "Incorrect lead cell association"
            );
            let mut row = vec![json!(lead)];
            for field in 0..fields.len() {
                let width = if field < nf { nl } else { 1 };
                let mut length = [0; 8];
                cursor.read_exact(&mut length)?;
                anyhow::ensure!(
                    u64::from_le_bytes(length) == (width * crate::packed::GROUPS) as u64,
                    "Malformed lead tile length"
                );
                let codes = packed_array(&mut cursor, point % crate::packed::LANES, 1)?;
                anyhow::ensure!(codes.len() == width, "Malformed lead profile length");
                let values = codes
                    .into_iter()
                    .enumerate()
                    .map(|(slot, code)| {
                        Ok(json!(
                            f32::from_bits(books[&lead].value(field, slot, code)?) as f64
                        ))
                    })
                    .collect::<Result<Vec<_>>>()?;
                row.push(if field < nf {
                    Value::Array(values)
                } else {
                    values.into_iter().next().unwrap()
                });
            }
            rows.push(Value::Array(row));
        }
        anyhow::ensure!(
            cursor.position() == bytes.len() as u64,
            "Unexpected lead profile rows"
        );
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
