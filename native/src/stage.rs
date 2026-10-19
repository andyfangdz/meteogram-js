//! Local immutable Float32 staging: independently checksummed Zstd frames,
//! indexed by a versioned JSON sidecar. No Python or array runtime is involved.
use crate::state::{atomic, load_npz, parse_npy, Npy};
use anyhow::{Context, Result};
use memmap2::MmapOptions;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
#[cfg(target_arch = "aarch64")]
use std::arch::aarch64::*;
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

pub(crate) const BLOCK_CELLS: usize = 4096;
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
    codes: Vec<u16>,
    decoder: zstd::bulk::Decompressor<'static>,
}
impl Scratch {
    pub fn new() -> Result<Self> {
        Ok(Self {
            compressed: Vec::new(),
            decoded: Vec::new(),
            codes: Vec::new(),
            decoder: zstd::bulk::Decompressor::new()?,
        })
    }
    fn decode(&mut self, file: &File, block: &Block, value_bytes: usize) -> Result<()> {
        self.compressed.resize(block.bytes, 0);
        file.read_exact_at(&mut self.compressed, block.offset)?;
        anyhow::ensure!(
            self.compressed.len() >= 5
                && self.compressed[..4] == [0x28, 0xb5, 0x2f, 0xfd]
                && self.compressed[4] & 4 != 0,
            "Staged block requires a checksummed Zstd frame"
        );
        let size = block.values * value_bytes;
        if self.decoded.len() < size {
            self.decoded.resize(size, 0);
        }
        let n = self
            .decoder
            .decompress_to_buffer(&self.compressed, &mut self.decoded[..size])?;
        anyhow::ensure!(n == size, "Invalid decoded block length");
        Ok(())
    }
}
// Reconstruct each spatial stream contiguously, then transpose small tiles.
// Partial reads use the same reconstruction and copy only requested values.
fn reconstruct_codes(bytes: &[u8], stride: usize, out: &mut [u16]) {
    assert!(stride > 0 && out.len().is_multiple_of(stride));
    assert_eq!(bytes.len(), out.len() * 2);
    let n = out.len();
    let cells = n / stride;
    for slot in 0..stride {
        let base = slot * cells;
        let low = &bytes[base..base + cells];
        let high = &bytes[n + base..n + base + cells];
        let target = &mut out[base..base + cells];
        let mut previous = 0u16;
        let mut cell = 0;
        #[cfg(target_arch = "aarch64")]
        unsafe {
            let zero = vdupq_n_u16(0);
            while cell + 8 <= cells {
                let mut value = vorrq_u16(
                    vmovl_u8(vld1_u8(low.as_ptr().add(cell))),
                    vshlq_n_u16(vmovl_u8(vld1_u8(high.as_ptr().add(cell))), 8),
                );
                value = vaddq_u16(value, vextq_u16(zero, value, 7));
                value = vaddq_u16(value, vextq_u16(zero, value, 6));
                value = vaddq_u16(value, vextq_u16(zero, value, 4));
                value = vaddq_u16(value, vdupq_n_u16(previous));
                vst1q_u16(target.as_mut_ptr().add(cell), value);
                previous = vgetq_lane_u16(value, 7);
                cell += 8;
            }
        }
        while cell < cells {
            previous = previous.wrapping_add(u16::from_le_bytes([low[cell], high[cell]]));
            target[cell] = previous;
            cell += 1;
        }
    }
}
fn transpose_codes(codes: &[u16], stride: usize, out: &mut [f32]) {
    assert!(stride > 0 && codes.len().is_multiple_of(stride));
    assert_eq!(codes.len(), out.len());
    let cells = codes.len() / stride;
    #[cfg(target_arch = "aarch64")]
    unsafe {
        let target = out.as_mut_ptr().cast::<u32>();
        for first in (0..cells).step_by(16) {
            let last = (first + 16).min(cells);
            let mut slot = 0;
            while slot + 4 <= stride {
                let mut cell = first;
                while cell + 4 <= last {
                    let a = vmovl_u16(vld1_u16(codes.as_ptr().add(slot * cells + cell)));
                    let b = vmovl_u16(vld1_u16(codes.as_ptr().add((slot + 1) * cells + cell)));
                    let c = vmovl_u16(vld1_u16(codes.as_ptr().add((slot + 2) * cells + cell)));
                    let d = vmovl_u16(vld1_u16(codes.as_ptr().add((slot + 3) * cells + cell)));
                    let ab0 = vtrn1q_u32(a, b);
                    let ab1 = vtrn2q_u32(a, b);
                    let cd0 = vtrn1q_u32(c, d);
                    let cd1 = vtrn2q_u32(c, d);
                    let rows = [
                        vcombine_u32(vget_low_u32(ab0), vget_low_u32(cd0)),
                        vcombine_u32(vget_low_u32(ab1), vget_low_u32(cd1)),
                        vcombine_u32(vget_high_u32(ab0), vget_high_u32(cd0)),
                        vcombine_u32(vget_high_u32(ab1), vget_high_u32(cd1)),
                    ];
                    for (k, row) in rows.into_iter().enumerate() {
                        vst1q_u32(target.add((cell + k) * stride + slot), row);
                    }
                    cell += 4;
                }
                for cell in cell..last {
                    for k in 0..4 {
                        target
                            .add(cell * stride + slot + k)
                            .write(u32::from(codes[(slot + k) * cells + cell]));
                    }
                }
                slot += 4;
            }
            for cell in first..last {
                for slot in slot..stride {
                    target
                        .add(cell * stride + slot)
                        .write(u32::from(codes[slot * cells + cell]));
                }
            }
        }
    }
    #[cfg(not(target_arch = "aarch64"))]
    for (cell, row) in out.chunks_exact_mut(stride).enumerate() {
        for (slot, value) in row.iter_mut().enumerate() {
            *value = f32::from_bits(u32::from(codes[slot * cells + cell]));
        }
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
    pub fn short_codes(&self) -> bool {
        matches!(&self.format,Format::Blocks(index) if index.version==2 && index.dtype=="uint16_le")
    }
    /// Complete aligned frames, retaining their [block][slot][cell] layout.
    pub fn read_codes_aligned(
        &self,
        start_cell: usize,
        cells: usize,
        out: &mut [u16],
        scratch: &mut Scratch,
    ) -> Result<u64> {
        anyhow::ensure!(
            self.short_codes(),
            "Direct codes require typed UInt16 staging"
        );
        let Format::Blocks(index) = &self.format else {
            unreachable!()
        };
        let stride = self.block_values / BLOCK_CELLS;
        let total_cells = self.values / stride;
        anyhow::ensure!(
            start_cell.is_multiple_of(BLOCK_CELLS)
                && cells > 0
                && start_cell <= total_cells
                && cells <= total_cells - start_cell
                && out.len() == cells * stride
                && ((start_cell + cells).is_multiple_of(BLOCK_CELLS)
                    || start_cell + cells == total_cells),
            "Unaligned direct code read"
        );
        let mut offset = 0;
        let mut failures = 0;
        for bi in start_cell / BLOCK_CELLS..(start_cell + cells).div_ceil(BLOCK_CELLS) {
            let block = &index.blocks[bi];
            scratch.decode(&self.file, block, 2)?;
            reconstruct_codes(
                &scratch.decoded[..block.values * 2],
                stride,
                &mut out[offset..offset + block.values],
            );
            offset += block.values;
            failures += discard(&self.file, block.offset, block.bytes);
        }
        anyhow::ensure!(offset == out.len(), "Incomplete direct code read");
        Ok(failures)
    }
    pub fn byte_len(&self) -> Result<u64> {
        Ok(self.file.metadata()?.len())
    }
    /// Read a bounded, immutable input into the OS cache before DB writes begin.
    /// The caller includes this pass in the load clock and memory budget. Every
    /// frame still receives its normal CRC/decode checks during ingestion.
    pub fn prefetch(&self, buffer: &mut [u8]) -> Result<()> {
        anyhow::ensure!(!buffer.is_empty(), "Empty prefetch buffer");
        let len = self.byte_len()?;
        let mut offset = 0;
        while offset < len {
            let bytes = buffer.len().min((len - offset) as usize);
            self.file.read_exact_at(&mut buffer[..bytes], offset)?;
            offset += bytes as u64;
        }
        Ok(())
    }
    pub fn format_name(&self) -> &'static str {
        match self.format {
            Format::Legacy { .. } => "legacy_npy_buffered_pread",
            Format::Blocks(ref index) if index.version == 2 => {
                "indexed_zstd_v2_spatial_delta16_buffered_pread"
            }
            Format::Blocks(_) => "indexed_zstd_v1_buffered_pread",
        }
    }
    pub fn open(logical: &Path, expected: &[usize]) -> Result<Self> {
        Self::open_as(logical, expected, "float32_le")
    }
    pub fn open_as(logical: &Path, expected: &[usize], dtype: &str) -> Result<Self> {
        let values = product(expected)?;
        let (data, index) = paths(logical);
        if data.exists() || index.exists() {
            let idx: Index = serde_json::from_reader(BufReader::new(File::open(index)?))?;
            let short_codes = dtype == "uint32_le"
                && idx.dtype == "uint16_le"
                && idx.version == 2
                && idx.encoding == "spatial_delta_shuffle16";
            anyhow::ensure!(
                ((idx.version == 1
                    && ["float32_le", "uint32_le"].contains(&idx.dtype.as_str())
                    && idx.dtype == dtype
                    && idx.encoding == "byte_shuffle32")
                    || short_codes)
                    && idx.shape == expected
                    && idx.block_cells == BLOCK_CELLS,
                "Invalid staged index"
            );
            let value_bytes = if short_codes { 2 } else { 4 };
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
                        && b.bytes <= block_values * value_bytes + 1024 * 1024
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
        anyhow::ensure!(
            dtype == "float32_le",
            "Dictionary indices require typed native staging"
        );
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
                let value_bytes = if index.dtype == "uint16_le" { 2 } else { 4 };
                let end = start + out.len();
                let mut failures = 0;
                for bi in start / self.block_values..=(end - 1) / self.block_values {
                    let b = &index.blocks[bi];
                    scratch.decode(&self.file, b, value_bytes)?;
                    let Scratch { decoded, codes, .. } = scratch;
                    let base = bi * self.block_values;
                    let lo = start.max(base);
                    let hi = end.min(base + b.values);
                    if value_bytes == 2 {
                        let stride = self.block_values / BLOCK_CELLS;
                        let cells = b.values / stride;
                        codes.resize(b.values, 0);
                        reconstruct_codes(&decoded[..b.values * 2], stride, codes);
                        if lo == base && hi == base + b.values {
                            transpose_codes(codes, stride, &mut out[lo - start..hi - start]);
                        } else {
                            for i in lo..hi {
                                let local = i - base;
                                out[i - start] = f32::from_bits(u32::from(
                                    codes[(local % stride) * cells + local / stride],
                                ));
                            }
                        }
                    } else {
                        for i in lo..hi {
                            let j = i - base;
                            out[i - start] = f32::from_bits(u32::from_le_bytes([
                                decoded[j],
                                decoded[b.values + j],
                                decoded[2 * b.values + j],
                                decoded[3 * b.values + j],
                            ]));
                        }
                    }
                    failures += discard(&self.file, b.offset, b.bytes);
                }
                Ok(failures)
            }
        }
    }
}
pub struct BlockWriter {
    file: File,
    logical: PathBuf,
    shape: Vec<usize>,
    dtype: String,
    count: usize,
    written: usize,
    block_values: usize,
    blocks: Vec<Block>,
    offset: u64,
    encoder: zstd::bulk::Compressor<'static>,
    shuffled: Vec<u8>,
}
impl BlockWriter {
    pub fn new(logical: &Path, shape: &[usize], dtype: &str) -> Result<Self> {
        anyhow::ensure!(
            ["float32_le", "uint32_le", "uint16_le"].contains(&dtype),
            "Unsupported staged dtype"
        );
        let count = product(shape)?;
        let stride = if shape.len() > 1 {
            product(&shape[1..])?
        } else {
            1
        };
        let block_values = BLOCK_CELLS.checked_mul(stride).context("Block overflow")?;
        let (data, _) = paths(logical);
        let file = File::create(data.with_extension("zst.tmp"))?;
        let mut encoder = zstd::bulk::Compressor::new(1)?;
        encoder.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
        Ok(Self {
            file,
            logical: logical.into(),
            shape: shape.into(),
            dtype: dtype.into(),
            count,
            written: 0,
            block_values,
            blocks: vec![],
            offset: 0,
            encoder,
            shuffled: vec![0u8; block_values * 4],
        })
    }
    pub fn append(&mut self, bits: &[u32]) -> Result<()> {
        let n = bits.len();
        anyhow::ensure!(
            self.written < self.count && n == self.block_values.min(self.count - self.written),
            "Incomplete or excess staged block"
        );
        let short = self.dtype == "uint16_le";
        if short {
            let stride = self.block_values / BLOCK_CELLS;
            let cells = n / stride;
            let mut previous = vec![0u16; stride];
            for cell in 0..cells {
                for (slot, previous) in previous.iter_mut().enumerate() {
                    let value = u16::try_from(bits[cell * stride + slot])
                        .context("UInt16 code overflow")?;
                    let delta = value.wrapping_sub(*previous).to_le_bytes();
                    *previous = value;
                    let j = slot * cells + cell;
                    self.shuffled[j] = delta[0];
                    self.shuffled[n + j] = delta[1];
                }
            }
        } else {
            for (i, v) in bits.iter().enumerate() {
                let b = v.to_le_bytes();
                for (k, byte) in b.iter().enumerate() {
                    self.shuffled[k * n + i] = *byte;
                }
            }
        }
        let encoded = self
            .encoder
            .compress(&self.shuffled[..n * if short { 2 } else { 4 }])?;
        self.file.write_all(&encoded)?;
        self.blocks.push(Block {
            offset: self.offset,
            bytes: encoded.len(),
            values: n,
        });
        self.offset += encoded.len() as u64;
        self.written += n;
        Ok(())
    }
    pub fn finish(self) -> Result<Value> {
        anyhow::ensure!(self.written == self.count, "Incomplete staged array");
        self.file.sync_all()?;
        let (data, index) = paths(&self.logical);
        fs::rename(data.with_extension("zst.tmp"), data)?;
        let short = self.dtype == "uint16_le";
        atomic(
            index,
            &Index {
                version: if short { 2 } else { 1 },
                dtype: self.dtype,
                shape: self.shape,
                block_cells: BLOCK_CELLS,
                encoding: if short {
                    "spatial_delta_shuffle16"
                } else {
                    "byte_shuffle32"
                }
                .into(),
                blocks: self.blocks,
            },
        )?;
        Ok(
            json!({"raw_bytes":self.count*if short { 2 } else { 4 },"encoded_bytes":self.offset,"format":if short { "indexed_zstd_v2_spatial_delta16" } else { "indexed_zstd_v1" },"checksum":true}),
        )
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
pub fn write_codes(logical: &Path, shape: &[usize], values: &[f32]) -> Result<Value> {
    anyhow::ensure!(values.len() == product(shape)?, "Code staging shape");
    let words = unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u32>(), values.len()) };
    let short = words.iter().all(|v| *v <= u16::MAX as u32);
    let mut writer = BlockWriter::new(
        logical,
        shape,
        if short { "uint16_le" } else { "uint32_le" },
    )?;
    for chunk in words.chunks(writer.block_values) {
        writer.append(chunk)?;
    }
    writer.finish()
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
    fn spatial_codes_reconstruct_every_slot_with_full_hrrr_strides() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for stride in [1, 2, 6, 180] {
            let logical = temp.path().join(format!("stride-{stride}.npy"));
            let values: Vec<_> = (0..4101 * stride)
                .map(|i| {
                    f32::from_bits(
                        (i as u32)
                            .wrapping_mul(2654435761)
                            .wrapping_add((i / stride) as u32)
                            & 65535,
                    )
                })
                .collect();
            write_codes(&logical, &[4101, stride], &values)?;
            let array = Array::open_as(&logical, &[4101, stride], "uint32_le")?;
            let mut out = vec![0f32; values.len()];
            array.read_into(0, &mut out)?;
            assert!(out
                .iter()
                .zip(&values)
                .all(|(a, b)| a.to_bits() == b.to_bits()));
            let start = 4095 * stride + 1;
            let len = (5 * stride).min(values.len() - start);
            let mut point = vec![0f32; len];
            array.read_into(start, &mut point)?;
            assert!(point
                .iter()
                .zip(&values[start..start + len])
                .all(|(a, b)| a.to_bits() == b.to_bits()));
        }
        Ok(())
    }
    #[test]
    fn spatial_delta_codes_cover_partial_blocks_wraps_and_u32_fallback() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let p = temp.path().join("lead-000.npy");
        let shape = [4101, 2, 3];
        let values: Vec<_> = (0..4101 * 6)
            .map(|i| f32::from_bits([0u32, 65535, 1, 32768, 17, 65400][(i + i / 6) % 6]))
            .collect();
        write_codes(&p, &shape, &values)?;
        let (_, index) = paths(&p);
        let idx: Value = crate::state::read_json(&index)?;
        assert_eq!(idx["version"], 2);
        assert_eq!(idx["dtype"], "uint16_le");
        assert!(Array::open(&p, &shape).is_err());
        let array = Array::open_as(&p, &shape, "uint32_le")?;
        assert!(array
            .read_codes_aligned(1, 4100, &mut vec![0u16; 4100 * 6], &mut Scratch::new()?)
            .is_err());
        assert!(array
            .read_codes_aligned(0, 4095, &mut vec![0u16; 4095 * 6], &mut Scratch::new()?)
            .is_err());
        for (start, len) in [
            (0, values.len()),
            (4090 * 6, 11 * 6),
            (37, 920),
            (values.len() - 1, 1),
        ] {
            let mut out = vec![0f32; len];
            array.read_into(start, &mut out)?;
            assert!(out
                .iter()
                .zip(&values[start..start + len])
                .all(|(a, b)| a.to_bits() == b.to_bits()));
        }
        let (data, _) = paths(&p);
        let mut bytes = fs::read(&data)?;
        bytes[20] ^= 1;
        fs::write(&data, bytes)?;
        assert!(Array::open_as(&p, &shape, "uint32_le")?
            .read_into(0, &mut [0f32; 6])
            .is_err());
        assert!(Array::open_as(&p, &shape, "uint32_le")?
            .read_codes_aligned(0, 4101, &mut vec![0u16; 4101 * 6], &mut Scratch::new()?)
            .is_err());
        let values = [f32::from_bits(65536), f32::from_bits(65535)];
        write_codes(&p, &[2], &values)?;
        assert_eq!(crate::state::read_json(index)?["dtype"], "uint32_le");
        let mut out = [0f32; 2];
        Array::open_as(&p, &[2], "uint32_le")?.read_into(0, &mut out)?;
        assert_eq!(out.map(f32::to_bits), [65536, 65535]);
        Ok(())
    }
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
