//! Local immutable Float32 staging: independently checksummed Zstd frames,
//! indexed by a versioned JSON sidecar. No Python or array runtime is involved.
use crate::state::{atomic, load_npz, parse_npy, Npy};
use anyhow::{Context, Result};
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(target_os = "linux")]
use std::os::fd::AsRawFd;
use std::{
    collections::BTreeMap,
    fs::{self, File},
    io::{BufReader, Read, Write},
    os::unix::fs::FileExt,
    path::{Path, PathBuf},
    time::Instant,
};

const BLOCK_CELLS: usize = 4096;
#[derive(Serialize, Deserialize)]
struct Block {
    offset: u64,
    bytes: usize,
    values: usize,
}
#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    dtype: String,
    shape: Vec<usize>,
    block_cells: usize,
    encoding: String,
    blocks: Vec<Block>,
}
enum Format {
    Legacy { offset: usize },
    Blocks(Index),
}
pub struct Array {
    file: File,
    format: Format,
    values: usize,
    block_values: usize,
}
pub struct Scratch {
    compressed: Vec<u8>,
    decoded: Vec<u8>,
    decoder: zstd::bulk::Decompressor<'static>,
}
impl Scratch {
    pub fn new() -> Result<Self> {
        Ok(Self {
            compressed: Vec::new(),
            decoded: Vec::new(),
            decoder: zstd::bulk::Decompressor::new()?,
        })
    }
}
fn paths(logical: &Path) -> (PathBuf, PathBuf) {
    (
        logical.with_extension("stage.zst"),
        logical.with_extension("stage.json"),
    )
}
fn product(shape: &[usize]) -> Result<usize> {
    anyhow::ensure!(
        !shape.is_empty() && shape.iter().all(|n| *n > 0),
        "Empty staged shape"
    );
    shape.iter().try_fold(1usize, |a, n| {
        a.checked_mul(*n).context("Staged shape overflow")
    })
}
pub fn exists(logical: &Path) -> bool {
    let (data, index) = paths(logical);
    (data.exists() && index.exists()) || logical.exists()
}
impl Array {
    pub fn format_name(&self) -> &'static str {
        match self.format {
            Format::Legacy { .. } => "legacy_npy_buffered_pread",
            Format::Blocks(_) => "indexed_zstd_v1_buffered_pread",
        }
    }
    pub fn open(logical: &Path, expected: &[usize]) -> Result<Self> {
        let values = product(expected)?;
        let (data, index) = paths(logical);
        if data.exists() || index.exists() {
            let idx: Index = serde_json::from_reader(BufReader::new(File::open(index)?))?;
            anyhow::ensure!(
                idx.version == 1
                    && idx.dtype == "float32_le"
                    && idx.shape == expected
                    && idx.block_cells == BLOCK_CELLS
                    && idx.encoding == "byte_shuffle32",
                "Invalid staged index"
            );
            let stride = if expected.len() > 1 {
                product(&expected[1..])?
            } else {
                1
            };
            let block_values = BLOCK_CELLS.checked_mul(stride).context("Block overflow")?;
            anyhow::ensure!(
                idx.blocks.len() == values.div_ceil(block_values),
                "Invalid staged block count"
            );
            let file = File::open(data)?;
            let mut offset = 0u64;
            for (i, b) in idx.blocks.iter().enumerate() {
                anyhow::ensure!(
                    b.offset == offset
                        && b.bytes > 0
                        && b.bytes <= block_values * 4 + 1024 * 1024
                        && b.values == block_values.min(values - i * block_values),
                    "Invalid staged block bounds"
                );
                offset = offset
                    .checked_add(b.bytes as u64)
                    .context("Offset overflow")?;
            }
            anyhow::ensure!(
                file.metadata()?.len() == offset,
                "Invalid staged file length"
            );
            return Ok(Self {
                file,
                format: Format::Blocks(idx),
                values,
                block_values,
            });
        }
        let file = File::open(logical)?;
        let map = unsafe { MmapOptions::new().map(&file)? };
        let (shape, dtype, offset) = parse_npy(&map)?;
        anyhow::ensure!(
            shape == expected
                && dtype == "<f4"
                && map.len() == offset + values * 4
                && offset % 4 == 0,
            "Invalid legacy staged array"
        );
        Ok(Self {
            file,
            format: Format::Legacy { offset },
            values,
            block_values: 0,
        })
    }
    pub fn read_into(&self, start: usize, out: &mut [f32]) -> Result<u64> {
        self.read_into_with(start, out, &mut Scratch::new()?)
    }
    pub fn read_into_with(
        &self,
        start: usize,
        out: &mut [f32],
        scratch: &mut Scratch,
    ) -> Result<u64> {
        anyhow::ensure!(
            start <= self.values && out.len() <= self.values - start,
            "Staged read out of bounds"
        );
        if out.is_empty() {
            return Ok(0);
        }
        match &self.format {
            Format::Legacy { offset, .. } => {
                let offset = offset + start * 4;
                let len = out.len() * 4;
                let bytes =
                    unsafe { std::slice::from_raw_parts_mut(out.as_mut_ptr().cast::<u8>(), len) };
                self.file.read_exact_at(bytes, offset as u64)?;
                Ok(discard(&self.file, offset as u64, len))
            }
            Format::Blocks(index) => {
                let Scratch {
                    compressed,
                    decoded,
                    decoder,
                } = scratch;
                if decoded.len() < self.block_values * 4 {
                    decoded.resize(self.block_values * 4, 0);
                }
                let end = start + out.len();
                let mut failures = 0;
                for bi in start / self.block_values..=(end - 1) / self.block_values {
                    let b = &index.blocks[bi];
                    compressed.resize(b.bytes, 0);
                    self.file.read_exact_at(compressed, b.offset)?;
                    anyhow::ensure!(
                        compressed.len() >= 5
                            && compressed[..4] == [0x28, 0xb5, 0x2f, 0xfd]
                            && compressed[4] & 4 != 0,
                        "Staged block requires a checksummed Zstd frame"
                    );
                    let len =
                        decoder.decompress_to_buffer(compressed, &mut decoded[..b.values * 4])?;
                    anyhow::ensure!(len == b.values * 4, "Invalid decoded block length");
                    let base = bi * self.block_values;
                    let lo = start.max(base);
                    let hi = end.min(base + b.values);
                    for i in lo..hi {
                        let j = i - base;
                        out[i - start] = f32::from_bits(u32::from_le_bytes([
                            decoded[j],
                            decoded[b.values + j],
                            decoded[2 * b.values + j],
                            decoded[3 * b.values + j],
                        ]));
                    }
                    failures += discard(&self.file, b.offset, b.bytes);
                }
                Ok(failures)
            }
        }
    }
}
fn discard(file: &File, offset: u64, len: usize) -> u64 {
    #[cfg(target_os = "linux")]
    return u64::from(
        unsafe {
            libc::posix_fadvise(
                file.as_raw_fd(),
                offset as libc::off_t,
                len as libc::off_t,
                libc::POSIX_FADV_DONTNEED,
            )
        } != 0,
    );
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, offset, len);
        0
    }
}
pub fn write(logical: &Path, shape: &[usize], values: &[f32]) -> Result<Value> {
    let count = product(shape)?;
    anyhow::ensure!(values.len() == count, "Staging shape mismatch");
    write_blocks(logical, shape, |start, out| {
        out.copy_from_slice(&values[start..start + out.len()]);
        Ok(())
    })
}
fn write_blocks<F>(logical: &Path, shape: &[usize], mut read: F) -> Result<Value>
where
    F: FnMut(usize, &mut [f32]) -> Result<()>,
{
    let started = Instant::now();
    let count = product(shape)?;
    let stride = if shape.len() > 1 {
        product(&shape[1..])?
    } else {
        1
    };
    let block_values = BLOCK_CELLS.checked_mul(stride).context("Block overflow")?;
    let (data, index) = paths(logical);
    let tmp = data.with_extension("zst.tmp");
    let mut file = File::create(&tmp)?;
    let mut encoder = zstd::bulk::Compressor::new(1)?;
    encoder.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
    let mut blocks = Vec::new();
    let mut offset = 0u64;
    let mut values = vec![0f32; block_values];
    let mut shuffled = vec![0u8; block_values * 4];
    for start in (0..count).step_by(block_values) {
        let n = block_values.min(count - start);
        read(start, &mut values[..n])?;
        for (i, value) in values[..n].iter().enumerate() {
            let b = value.to_bits().to_le_bytes();
            for k in 0..4 {
                shuffled[k * n + i] = b[k];
            }
        }
        let encoded = encoder.compress(&shuffled[..n * 4])?;
        file.write_all(&encoded)?;
        blocks.push(Block {
            offset,
            bytes: encoded.len(),
            values: n,
        });
        offset += encoded.len() as u64;
    }
    file.sync_all()?;
    fs::rename(tmp, data)?;
    atomic(
        index,
        &Index {
            version: 1,
            dtype: "float32_le".into(),
            shape: shape.to_vec(),
            block_cells: BLOCK_CELLS,
            encoding: "byte_shuffle32".into(),
            blocks,
        },
    )?;
    Ok(
        json!({"raw_bytes":count*4,"encoded_bytes":offset,"seconds":started.elapsed().as_secs_f64(),"format":"indexed_zstd_v1","checksum":true}),
    )
}
pub fn load_cells(dir: &Path) -> Result<BTreeMap<String, Npy>> {
    let p = dir.join("cells.bin.zst");
    if !p.exists() {
        return load_npz(&dir.join("cells.npz"));
    }
    let file = File::open(p)?;
    let mut frame = [0u8; 5];
    file.read_exact_at(&mut frame, 0)?;
    anyhow::ensure!(
        frame[..4] == [0x28, 0xb5, 0x2f, 0xfd] && frame[4] & 4 != 0,
        "Cell index requires a checksummed Zstd frame"
    );
    let mut reader = zstd::stream::read::Decoder::new(file)?;
    let mut header = [0u8; 16];
    reader.read_exact(&mut header)?;
    anyhow::ensure!(&header[..8] == b"MGCELL01", "Invalid cell index version");
    let n = usize::try_from(u64::from_le_bytes(header[8..].try_into()?))?;
    anyhow::ensure!(n > 0 && n <= 10_000_000, "Invalid cell count");
    let mut result = BTreeMap::new();
    for (name, dtype, size) in [
        ("cell_id", "<u4", 4),
        ("latitude", "<f8", 8),
        ("longitude", "<f8", 8),
    ] {
        let mut bytes = vec![0u8; n * size];
        reader.read_exact(&mut bytes)?;
        result.insert(
            name.into(),
            Npy {
                dtype: dtype.into(),
                bytes,
            },
        );
    }
    let mut tail = [0u8; 1];
    anyhow::ensure!(reader.read(&mut tail)? == 0, "Trailing cell index data");
    Ok(result)
}
pub fn save_cells(dir: &Path, ids: &[u32], lat: &[f64], lon: &[f64]) -> Result<()> {
    anyhow::ensure!(
        !ids.is_empty() && ids.len() == lat.len() && ids.len() == lon.len(),
        "Cell index lengths"
    );
    let path = dir.join("cells.bin.zst");
    let tmp = dir.join("cells.bin.zst.tmp");
    let mut encoder = zstd::stream::write::Encoder::new(File::create(&tmp)?, 1)?;
    encoder.include_checksum(true)?;
    encoder.write_all(b"MGCELL01")?;
    encoder.write_all(&(ids.len() as u64).to_le_bytes())?;
    for v in ids {
        encoder.write_all(&v.to_le_bytes())?;
    }
    for v in lat {
        encoder.write_all(&v.to_le_bytes())?;
    }
    for v in lon {
        encoder.write_all(&v.to_le_bytes())?;
    }
    encoder.finish()?.sync_all()?;
    fs::rename(tmp, path)?;
    File::open(dir)?.sync_all()?;
    Ok(())
}
pub fn copy_cells(source: &Path, target: &Path) -> Result<()> {
    let name = if source.join("cells.bin.zst").exists() {
        "cells.bin.zst"
    } else {
        "cells.npz"
    };
    fs::copy(source.join(name), target.join(name))?;
    Ok(())
}
pub fn convert(source: &Path, target: &Path) -> Result<Value> {
    anyhow::ensure!(
        source != target && !target.exists(),
        "Conversion needs a fresh output directory"
    );
    fs::create_dir_all(target)?;
    let meta: Value =
        serde_json::from_reader(BufReader::new(File::open(source.join("metadata.json"))?))?;
    let nc = meta["cells"].as_u64().context("Cells")? as usize;
    let nl = meta["levels"].as_array().context("Levels")?.len();
    let nf = meta["variables"].as_array().context("Fields")?.len();
    let started = Instant::now();
    let mut reports = Vec::new();
    for lead in meta["leads"].as_array().context("Leads")? {
        let lead = lead.as_u64().context("Lead")?;
        for (name, shape) in [
            (format!("lead-{lead:03}.npy"), vec![nc, nl, nf]),
            (format!("surface-{lead:03}.npy"), vec![nc, 2]),
        ] {
            let array = Array::open(&source.join(&name), &shape)?;
            let report = write_blocks(&target.join(&name), &shape, |start, out| {
                array.read_into(start, out)?;
                Ok(())
            })?;
            reports.push(report);
        }
        println!("{}", json!({"phase":"convert_stage","lead":lead}));
    }
    let cells = load_cells(source)?;
    let ids: Vec<_> = cells["cell_id"]
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let lat: Vec<_> = cells["latitude"]
        .bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f64::from_le_bytes(*b))
        .collect();
    let lon: Vec<_> = cells["longitude"]
        .bytes
        .as_chunks::<8>()
        .0
        .iter()
        .map(|b| f64::from_le_bytes(*b))
        .collect();
    save_cells(target, &ids, &lat, &lon)?;
    atomic(target.join("metadata.json"), &meta)?;
    let report = json!({"arrays":reports.len(),"raw_bytes":reports.iter().map(|v|v["raw_bytes"].as_u64().unwrap()).sum::<u64>(),"encoded_bytes":reports.iter().map(|v|v["encoded_bytes"].as_u64().unwrap()).sum::<u64>(),"seconds":started.elapsed().as_secs_f64(),"blocks":"4096 cells","format":"indexed_zstd_v1"});
    atomic(target.join("conversion.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn bounded_stage_preserves_every_bit_and_checks_corruption() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let p = temp.path().join("lead-000.npy");
        let n = 4101 * 6;
        let values: Vec<_> = (0..n)
            .map(|i| {
                f32::from_bits(
                    [
                        0, 0x80000000, 1, 0x7f7fffff, 0x7fc00001, 0xff800000, i as u32,
                    ][i % 7],
                )
            })
            .collect();
        write(&p, &[4101, 2, 3], &values)?;
        assert!(!p.exists());
        let array = Array::open(&p, &[4101, 2, 3])?;
        for (start, len) in [(0, n), (4090 * 6, 11 * 6), (37, 920), (n - 1, 1)] {
            let mut out = vec![0f32; len];
            array.read_into(start, &mut out)?;
            assert!(out
                .iter()
                .zip(&values[start..start + len])
                .all(|(a, b)| a.to_bits() == b.to_bits()));
        }
        assert!(array.read_into(n, &mut [0f32; 1]).is_err());
        assert!(Array::open(&p, &[4101, 3, 2]).is_err());
        let (data, index) = paths(&p);
        let mut b = fs::read(&data)?;
        b[20] ^= 1;
        fs::write(&data, &b)?;
        assert!(Array::open(&p, &[4101, 2, 3])?
            .read_into(0, &mut [0f32; 6])
            .is_err());
        b.pop();
        fs::write(&data, b)?;
        assert!(Array::open(&p, &[4101, 2, 3]).is_err());
        fs::remove_file(index)?;
        assert!(Array::open(&p, &[4101, 2, 3]).is_err());
        Ok(())
    }
    #[test]
    fn cells_native_and_legacy_match_and_check_truncation() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let ids = [3, 8];
        let lat = [-0.0, 49.333];
        let lon = [-120.123, 1e-10];
        crate::state::save_cells(&temp.path().join("cells.npz"), &ids, &lat, &lon)?;
        let old = load_cells(temp.path())?;
        save_cells(temp.path(), &ids, &lat, &lon)?;
        let new = load_cells(temp.path())?;
        for k in old.keys() {
            assert_eq!(old[k].dtype, new[k].dtype);
            assert_eq!(old[k].bytes, new[k].bytes);
        }
        let p = temp.path().join("cells.bin.zst");
        let mut b = fs::read(&p)?;
        b.pop();
        fs::write(p, b)?;
        assert!(load_cells(temp.path()).is_err());
        Ok(())
    }
}
