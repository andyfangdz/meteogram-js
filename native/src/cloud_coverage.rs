//! Exact cloud fractions in small independently checksummed spatial frames.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, fs::File, io::Write, os::unix::fs::FileExt, path::Path};

pub const LEVELS: usize = 50;
const CELLS: usize = 256;
const MAX_CELLS: usize = 2_000_000;
pub const DATA: &str = "cloud-coverage.bin.zst";
pub const INDEX: &str = "cloud-coverage.json";

pub enum Codes {
    Constant(usize),
    Byte(Vec<u8>),
    Short(Vec<u16>),
}
pub struct Level {
    pub palette: Vec<u32>,
    codes: Codes,
}
impl Codes {
    fn push(&mut self, code: u16) {
        match self {
            Self::Constant(_) => unreachable!("constant levels are immutable"),
            Self::Byte(values) if code <= 255 => values.push(code as u8),
            Self::Byte(values) => {
                let mut wide: Vec<u16> =
                    std::mem::take(values).into_iter().map(u16::from).collect();
                wide.push(code);
                *self = Self::Short(wide);
            }
            Self::Short(values) => values.push(code),
        }
    }
    fn len(&self) -> usize {
        match self {
            Self::Constant(n) => *n,
            Self::Byte(v) => v.len(),
            Self::Short(v) => v.len(),
        }
    }
    fn get(&self, cell: usize) -> u16 {
        match self {
            Self::Constant(_) => 0,
            Self::Byte(v) => u16::from(v[cell]),
            Self::Short(v) => v[cell],
        }
    }
}
pub fn encode(values: impl ExactSizeIterator<Item = f32> + Clone) -> Result<Level> {
    anyhow::ensure!((1..=MAX_CELLS).contains(&values.len()), "Cloud cell count");
    let first = values.clone().next().context("Empty cloud level")?;
    if values.clone().all(|v| v.to_bits() == first.to_bits()) {
        anyhow::ensure!(
            first.is_finite() && (0. ..=1.).contains(&first),
            "Invalid constant cloud fraction"
        );
        return Ok(Level {
            palette: vec![first.to_bits()],
            codes: Codes::Constant(values.len()),
        });
    }
    // These are source Float32 values, not rounded replacements. The bit check
    // below sends every other fraction (including negative zero) to the exact
    // extension palette rather than quantizing it.
    let mut palette: Vec<u32> = (0..=100)
        .map(|p| (f64::from(p) / 100.) as f32)
        .map(f32::to_bits)
        .collect();
    let mut extra = HashMap::new();
    let mut codes = Codes::Byte(Vec::with_capacity(values.len()));
    for value in values {
        anyhow::ensure!(
            value.is_finite() && (0. ..=1.).contains(&value),
            "Invalid cloud fraction"
        );
        let bits = value.to_bits();
        let percent = (value * 100.).round() as usize;
        let code = if palette[percent] == bits {
            percent as u16
        } else if let Some(code) = extra.get(&bits) {
            *code
        } else {
            let code = u16::try_from(palette.len()).context("Cloud palette exceeds UInt16")?;
            palette.push(bits);
            extra.insert(bits, code);
            code
        };
        codes.push(code);
    }
    Ok(Level { palette, codes })
}

#[derive(Serialize, Deserialize)]
struct Frame {
    offset: u64,
    bytes: usize,
    cells: usize,
}
#[derive(Serialize, Deserialize)]
struct Index {
    version: u32,
    cells: usize,
    levels: usize,
    block_cells: usize,
    palettes: Vec<Vec<u32>>,
    widths: Vec<usize>,
    frames: Vec<Frame>,
}
pub fn write(path: &Path, levels: &[Level]) -> Result<Value> {
    let started = std::time::Instant::now();
    anyhow::ensure!(levels.len() == LEVELS, "Expected every native cloud level");
    let cells = levels[0].codes.len();
    anyhow::ensure!(
        (1..=MAX_CELLS).contains(&cells) && levels.iter().all(|l| l.codes.len() == cells),
        "Cloud axes differ"
    );
    let widths: Vec<_> = levels
        .iter()
        .map(|l| match l.codes {
            Codes::Constant(_) => 0,
            Codes::Byte(_) => 1,
            Codes::Short(_) => 2,
        })
        .collect();
    let mut encoder = zstd::bulk::Compressor::new(1)?;
    encoder.set_parameter(zstd::zstd_safe::CParameter::ChecksumFlag(true))?;
    let temporary = path.join(format!("{DATA}.tmp"));
    let mut file = File::create(&temporary)?;
    let mut frames = Vec::new();
    let mut offset = 0;
    for first in (0..cells).step_by(CELLS) {
        let n = CELLS.min(cells - first);
        let mut raw = Vec::with_capacity(n * widths.iter().sum::<usize>());
        for (level, width) in levels.iter().zip(&widths) {
            if *width == 0 {
                continue;
            }
            let start = raw.len();
            raw.resize(start + n * width, 0);
            let mut prior = 0u16;
            for c in 0..n {
                let code = level.codes.get(first + c);
                let delta = code.wrapping_sub(prior);
                prior = code;
                raw[start + c] = delta as u8;
                if *width == 2 {
                    raw[start + n + c] = (delta >> 8) as u8;
                }
            }
        }
        let compressed = encoder.compress(&raw)?;
        file.write_all(&compressed)?;
        frames.push(Frame {
            offset,
            bytes: compressed.len(),
            cells: n,
        });
        offset += compressed.len() as u64;
    }
    file.sync_all()?;
    fs::rename(temporary, path.join(DATA))?;
    crate::state::atomic(
        path.join(INDEX),
        &Index {
            version: 1,
            cells,
            levels: LEVELS,
            block_cells: CELLS,
            palettes: levels.iter().map(|l| l.palette.clone()).collect(),
            widths,
            frames,
        },
    )?;
    let reader = Reader::open(path, cells)?;
    let mut scratch = Scratch::new()?;
    for first in (0..cells).step_by(CELLS) {
        reader.frame(first / CELLS, &mut scratch)?;
        for (level, source) in levels.iter().enumerate() {
            for c in 0..CELLS.min(cells - first) {
                anyhow::ensure!(
                    scratch.codes[level * scratch.cells + c] == source.codes.get(first + c),
                    "Cloud stored index differs from source"
                );
            }
        }
    }
    Ok(
        json!({"format":"cloud_palette_spatial_delta_v1","data_bytes":offset,
        "index_bytes":fs::metadata(path.join(INDEX))?.len(),"cells":cells,"levels":LEVELS,
        "palette_entries":levels.iter().map(|l|l.palette.len()).sum::<usize>(),
        "full_values_checked":true,"seconds":started.elapsed().as_secs_f64()}),
    )
}
pub struct Reader {
    file: File,
    index: Index,
}
pub struct Scratch {
    input: Vec<u8>,
    raw: Vec<u8>,
    codes: Vec<u16>,
    cells: usize,
    decoder: zstd::bulk::Decompressor<'static>,
}
impl Scratch {
    pub fn new() -> Result<Self> {
        Ok(Self {
            input: Vec::new(),
            raw: Vec::new(),
            codes: Vec::new(),
            cells: 0,
            decoder: zstd::bulk::Decompressor::new()?,
        })
    }
}
impl Reader {
    pub fn open(path: &Path, cells: usize) -> Result<Self> {
        anyhow::ensure!((1..=MAX_CELLS).contains(&cells), "Invalid cloud cell count");
        let metadata = path.join(INDEX);
        anyhow::ensure!(
            fs::metadata(&metadata)?.len() <= 48 * 1024 * 1024,
            "Cloud index too large"
        );
        let index: Index = serde_json::from_slice(&fs::read(metadata)?)?;
        anyhow::ensure!(
            index.version == 1
                && index.cells == cells
                && index.levels == LEVELS
                && index.block_cells == CELLS
                && index.palettes.len() == LEVELS
                && index.widths.len() == LEVELS
                && index.frames.len() == cells.div_ceil(CELLS),
            "Invalid cloud coverage index"
        );
        for (palette, width) in index.palettes.iter().zip(&index.widths) {
            anyhow::ensure!(
                !palette.is_empty()
                    && palette.len() <= 65536
                    && [0, 1, 2].contains(width)
                    && (*width != 0 || palette.len() == 1)
                    && (*width == 2 || palette.len() <= 256)
                    && palette.iter().all(|bits| {
                        let v = f32::from_bits(*bits);
                        v.is_finite() && (0. ..=1.).contains(&v)
                    }),
                "Invalid cloud coverage palette"
            );
        }
        let file = File::open(path.join(DATA))?;
        let mut end = 0u64;
        for (i, f) in index.frames.iter().enumerate() {
            anyhow::ensure!(
                f.offset == end
                    && f.cells == CELLS.min(cells - i * CELLS)
                    && f.bytes >= 5
                    && f.bytes <= CELLS * LEVELS * 2 + 1024,
                "Invalid cloud frame bounds"
            );
            end = end
                .checked_add(u64::try_from(f.bytes)?)
                .context("Cloud frame overflow")?;
        }
        anyhow::ensure!(end == file.metadata()?.len(), "Cloud coverage file length");
        Ok(Self { file, index })
    }
    fn frame(&self, frame: usize, scratch: &mut Scratch) -> Result<()> {
        let f = &self.index.frames[frame];
        scratch.input.resize(f.bytes, 0);
        self.file.read_exact_at(&mut scratch.input, f.offset)?;
        anyhow::ensure!(
            scratch.input[..4] == [0x28, 0xb5, 0x2f, 0xfd] && scratch.input[4] & 4 != 0,
            "Cloud frame requires checksum"
        );
        let bytes = f.cells * self.index.widths.iter().sum::<usize>();
        scratch.raw.resize(bytes, 0);
        let n = scratch
            .decoder
            .decompress_to_buffer(&scratch.input, &mut scratch.raw[..])?;
        anyhow::ensure!(n == bytes, "Cloud decoded length");
        scratch.codes.resize(f.cells * LEVELS, 0);
        scratch.cells = f.cells;
        let mut base = 0;
        for (level, width) in self.index.widths.iter().enumerate() {
            if *width == 0 {
                scratch.codes[level * f.cells..(level + 1) * f.cells].fill(0);
                continue;
            }
            let mut prior = 0u16;
            for cell in 0..f.cells {
                let low = u16::from(scratch.raw[base + cell]);
                let delta = if *width == 1 {
                    low
                } else {
                    low | (u16::from(scratch.raw[base + f.cells + cell]) << 8)
                };
                prior = if *width == 1 {
                    u16::from((prior as u8).wrapping_add(delta as u8))
                } else {
                    prior.wrapping_add(delta)
                };
                anyhow::ensure!(
                    usize::from(prior) < self.index.palettes[level].len(),
                    "Cloud palette index out of bounds"
                );
                scratch.codes[level * f.cells + cell] = prior;
            }
            base += f.cells * width;
        }
        Ok(())
    }
    pub fn profile(&self, cell: usize, scratch: &mut Scratch) -> Result<Vec<f32>> {
        anyhow::ensure!(cell < self.index.cells, "Cloud cell out of bounds");
        self.frame(cell / CELLS, scratch)?;
        Ok((0..LEVELS)
            .map(|l| {
                f32::from_bits(
                    self.index.palettes[l]
                        [usize::from(scratch.codes[l * scratch.cells + cell % CELLS])],
                )
            })
            .collect())
    }
    pub fn read_into(&self, first: usize, values: &mut [f32], scratch: &mut Scratch) -> Result<()> {
        anyhow::ensure!(values.len().is_multiple_of(LEVELS), "Cloud output shape");
        let cells = values.len() / LEVELS;
        anyhow::ensure!(
            first <= self.index.cells && cells <= self.index.cells - first,
            "Cloud range out of bounds"
        );
        if cells == 0 {
            return Ok(());
        }
        for frame in first / CELLS..(first + cells).div_ceil(CELLS) {
            self.frame(frame, scratch)?;
            let begin = first.max(frame * CELLS);
            let end = (first + cells).min(frame * CELLS + scratch.cells);
            for cell in begin..end {
                for l in 0..LEVELS {
                    values[(cell - first) * LEVELS + l] = f32::from_bits(
                        self.index.palettes[l]
                            [usize::from(scratch.codes[l * scratch.cells + cell % CELLS])],
                    );
                }
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn exact_fraction_bits_variable_width_and_short_final_frame() {
        let root = tempfile::tempdir().unwrap();
        let source: Vec<Vec<f32>> = (0..LEVELS)
            .map(|l| {
                (0..519)
                    .map(|c| {
                        if l == 0 {
                            f32::from_bits(0x3e800000 + c as u32)
                        } else if c == 0 {
                            -0.
                        } else {
                            (c % 101) as f32 / 100.
                        }
                    })
                    .collect()
            })
            .collect();
        let levels: Vec<_> = source
            .iter()
            .map(|s| encode(s.iter().copied()).unwrap())
            .collect();
        assert!(matches!(levels[0].codes, Codes::Short(_)));
        write(root.path(), &levels).unwrap();
        let reader = Reader::open(root.path(), 519).unwrap();
        let mut scratch = Scratch::new().unwrap();
        for cell in [0, 255, 256, 511, 512, 518] {
            let actual = reader.profile(cell, &mut scratch).unwrap();
            for l in 0..LEVELS {
                assert_eq!(actual[l].to_bits(), source[l][cell].to_bits());
            }
        }
        assert!(reader.profile(519, &mut scratch).is_err());
    }
    #[test]
    fn rejects_corrupt_frames_and_nonfinite_fractions() {
        for bad in [f32::NAN, f32::INFINITY, -0.01, 1.01] {
            assert!(encode([bad].into_iter()).is_err());
        }
        let root = tempfile::tempdir().unwrap();
        let levels: Vec<_> = (0..LEVELS)
            .map(|_| encode([0., 0.5, 1.].into_iter()).unwrap())
            .collect();
        write(root.path(), &levels).unwrap();
        let mut bytes = fs::read(root.path().join(DATA)).unwrap();
        let last = bytes.len() - 1;
        bytes[last] ^= 1;
        fs::write(root.path().join(DATA), bytes).unwrap();
        let reader = Reader::open(root.path(), 3).unwrap();
        assert!(reader.profile(0, &mut Scratch::new().unwrap()).is_err());
    }
    #[test]
    fn constant_planes_keep_signed_zero_and_read_across_frame_edges() {
        let root = tempfile::tempdir().unwrap();
        let values: Vec<_> = (0..LEVELS)
            .map(|l| if l % 2 == 0 { -0. } else { 1. })
            .collect();
        let levels: Vec<_> = values
            .iter()
            .map(|v| encode(std::iter::repeat_n(*v, 263)).unwrap())
            .collect();
        write(root.path(), &levels).unwrap();
        let reader = Reader::open(root.path(), 263).unwrap();
        assert!(reader.index.widths.iter().all(|w| *w == 0));
        let mut actual = vec![0.; 10 * LEVELS];
        reader
            .read_into(253, &mut actual, &mut Scratch::new().unwrap())
            .unwrap();
        for cell in actual.as_chunks::<LEVELS>().0 {
            assert!(cell
                .iter()
                .zip(&values)
                .all(|(a, b)| a.to_bits() == b.to_bits()));
        }
    }
    #[tokio::test(flavor = "multi_thread", worker_threads = 4)]
    #[ignore = "Requires an explicit resident source hour and isolated output directory"]
    async fn full_conus_live_source_cost_and_exactness() -> anyhow::Result<()> {
        let paths = std::env::var_os("METEOGRAM_CLOUD_COST_SOURCE").context("Source hours")?;
        let sources: Vec<_> = std::env::split_paths(&paths).collect();
        anyhow::ensure!(!sources.is_empty(), "Missing source hours");
        let output = std::path::PathBuf::from(std::env::var("METEOGRAM_CLOUD_COST_OUTPUT")?);
        anyhow::ensure!(!output.exists(), "Cost output must be fresh");
        fs::create_dir_all(&output)?;
        async fn measured(
            path: &Path,
            pin: &Value,
            lead: u16,
            src: &crate::source::Source,
            clouds: bool,
        ) -> anyhow::Result<(Value, f64)> {
            let started = std::time::Instant::now();
            let meta = if clouds {
                crate::source::fetch_hour("hrrr", path, pin.clone(), lead, src).await?
            } else {
                crate::source::fetch_pressure_hour("hrrr", path, pin.clone(), lead, src).await?
            };
            Ok((meta, started.elapsed().as_secs_f64()))
        }
        let mut reports = Vec::new();
        for (sample_index, source) in sources.into_iter().enumerate() {
            let meta: Value = serde_json::from_slice(&fs::read(source.join("metadata.json"))?)?;
            let mut pin = meta.clone();
            pin["snapshot_id"] = meta["native_clouds"]["snapshot_id"].clone();
            let lead = meta["leads"][0].as_u64().context("Lead")? as u16;
            let output = output.join(format!("hour-{lead:03}"));
            anyhow::ensure!(!output.exists(), "Duplicate source hour");
            fs::create_dir_all(&output)?;
            // Availability normally warms weather references before ingestion.
            // Prime only weather here; no cloud map or reference is read.
            // Weather payload caching is disabled in Icechunk. Both timed runs
            // download fresh payloads into independent staging directories.
            let primer = crate::source::Source::open("hrrr", Some(&pin)).await?;
            crate::source::fetch_pressure_hour(
                "hrrr",
                &output.join("primer"),
                pin.clone(),
                lead,
                &primer,
            )
            .await?;
            fs::remove_dir_all(output.join("primer"))?;
            let control_source = crate::source::Source::open("hrrr", Some(&pin)).await?;
            let candidate_source = crate::source::Source::open("hrrr", Some(&pin)).await?;
            let coverage = output.join("integrated");
            let (control, integrated) = if sample_index % 2 == 1 {
                let candidate = measured(&coverage, &pin, lead, &candidate_source, true).await?;
                let control =
                    measured(&output.join("pressure"), &pin, lead, &control_source, false).await?;
                (control, candidate)
            } else {
                let control =
                    measured(&output.join("pressure"), &pin, lead, &control_source, false).await?;
                let candidate = measured(&coverage, &pin, lead, &candidate_source, true).await?;
                (control, candidate)
            };
            let (_, pressure_seconds) = control;
            let (integrated_meta, integrated_seconds) = integrated;
            let info = integrated_meta["native_clouds"].clone();
            let coverage_wall_seconds = info["total_seconds"].as_f64().context("Cloud clock")?;
            let nc = meta["cells"].as_u64().context("Cells")? as usize;
            let old = crate::stage::Array::open(&source.join("native-clouds"), &[nc, 50, 3])?;
            let new = Reader::open(&coverage, nc)?;
            let anchors = crate::stage::Array::open(&coverage.join("cloud-anchors"), &[nc, 2])?;
            let pressure_path = output.join("pressure");
            for name in [
                format!("lead-{lead:03}.stage.zst"),
                format!("lead-{lead:03}.stage.json"),
                format!("surface-{lead:03}.stage.zst"),
                format!("surface-{lead:03}.stage.json"),
                "values.dict.zst".into(),
                "cells.bin.zst".into(),
            ] {
                anyhow::ensure!(
                    crate::dictionary::file_hash(&pressure_path.join(&name))?
                        == crate::dictionary::file_hash(&coverage.join(&name))?,
                    "Parallel cloud ingestion changed weather bytes: {name}"
                );
            }
            let pressure_meta = crate::state::read_json(pressure_path.join("metadata.json"))?;
            let dictionary = crate::dictionary::Dictionary::load(&pressure_path, &pressure_meta)?;
            let weather = crate::stage::Array::open_as(
                &pressure_path.join(format!("lead-{lead:03}")),
                &[nc, 30, 6],
                "uint16_le",
            )?;
            let pressure_levels: Vec<f64> =
                serde_json::from_value(pressure_meta["levels"].clone())?;
            let mut old_scratch = crate::stage::Scratch::new()?;
            let mut new_scratch = Scratch::new()?;
            let mut anchor_scratch = crate::stage::Scratch::new()?;
            let mut weather_scratch = crate::stage::Scratch::new()?;
            let mut before = vec![0.; 4096 * 150];
            let mut after = vec![0.; 4096 * 50];
            let mut anchor_values = vec![0.; 4096 * 2];
            let mut weather_values = vec![0f32; 4096 * 180];
            let mut heights = [0.; 30];
            let mut dewpoints = [0.; 30];
            let mut height_histogram = vec![0u64; 10001];
            let mut pressure_histogram = vec![0u64; 10001];
            let mut compared_geometry = 0u64;
            let mut max_height_error = 0f64;
            let mut max_pressure_error = 0f64;
            let mut max_height_error_all_levels = 0f64;
            let mut height_bias = 0.;
            for first in (0..nc).step_by(4096) {
                let n = 4096.min(nc - first);
                old.read_into_with(first * 150, &mut before[..n * 150], &mut old_scratch)?;
                new.read_into(first, &mut after[..n * 50], &mut new_scratch)?;
                anchors.read_into_with(
                    first * 2,
                    &mut anchor_values[..n * 2],
                    &mut anchor_scratch,
                )?;
                weather.read_into_with(
                    first * 180,
                    &mut weather_values[..n * 180],
                    &mut weather_scratch,
                )?;
                for c in 0..n {
                    let anchor = [anchor_values[c * 2], anchor_values[c * 2 + 1]];
                    anyhow::ensure!(
                        anchor[0].to_bits() == before[c * 150 + 2].to_bits()
                            && anchor[1].to_bits() == before[c * 150 + 148].to_bits(),
                        "Native anchor changed at cell {}",
                        first + c
                    );
                    for l in 0..30 {
                        let offset = c * 180 + l * 6;
                        heights[l] = f32::from_bits(dictionary.value(
                            0,
                            l,
                            weather_values[offset].to_bits(),
                        )?) as f64;
                        dewpoints[l] = f32::from_bits(dictionary.value(
                            5,
                            l,
                            weather_values[offset + 5].to_bits(),
                        )?) as f64;
                    }
                    let [derived_height, derived_pressure] = crate::cloud_geometry::derive(
                        anchor,
                        &pressure_levels,
                        &heights,
                        &dewpoints,
                    )?;
                    for l in 0..50 {
                        anyhow::ensure!(
                            before[c * 150 + l * 3].to_bits() == after[c * 50 + l].to_bits(),
                            "Source cloud differs at cell {}, level {}",
                            first + c,
                            l
                        );
                        let original_height = f64::from(before[c * 150 + l * 3 + 1]);
                        let height_error = f64::from(derived_height[l]) - original_height;
                        max_height_error_all_levels =
                            max_height_error_all_levels.max(height_error.abs());
                        if (0. ..=6096.).contains(&original_height) {
                            let pressure_error = (f64::from(derived_pressure[l])
                                - f64::from(before[c * 150 + l * 3 + 2]))
                            .abs();
                            height_histogram
                                [((height_error.abs() * 10.).floor() as usize).min(10000)] += 1;
                            pressure_histogram
                                [((pressure_error * 10.).floor() as usize).min(10000)] += 1;
                            compared_geometry += 1;
                            max_height_error = max_height_error.max(height_error.abs());
                            max_pressure_error = max_pressure_error.max(pressure_error);
                            height_bias += height_error;
                        }
                    }
                }
            }
            let percentile = |histogram: &[u64], fraction: f64| -> f64 {
                let target = (compared_geometry as f64 * fraction).ceil() as u64;
                let mut count = 0;
                for (bin, n) in histogram.iter().enumerate() {
                    count += n;
                    if count >= target {
                        return (bin + 1) as f64 / 10.;
                    }
                }
                f64::NAN
            };
            let report = json!({"lead":lead,"measurement_order":if sample_index % 2 == 1 {"integrated_then_control"} else {"control_then_integrated"},"full_conus_coverage_values_compared":nc*50,"all_fractions_exact":true,"all_anchors_exact":true,"all_derived_columns_finite_and_monotonic":true,
                "pressure_source_seconds":pressure_seconds,"pressure_and_cloud_source_seconds":integrated_seconds,"added_cloud_source_seconds":integrated_seconds-pressure_seconds,"cloud_branch_wall_seconds":coverage_wall_seconds,"cloud":info,"all_weather_staging_bytes_identical":true,
                "geometry_comparison":{"height_range_m":[0,6096],"values":compared_geometry,"height_abs_error_p50_upper_m":percentile(&height_histogram,0.5),"height_abs_error_p99_upper_m":percentile(&height_histogram,0.99),"height_abs_error_max_m":max_height_error,"height_bias_m":height_bias/compared_geometry as f64,"pressure_abs_error_p99_upper_pa":percentile(&pressure_histogram,0.99),"pressure_abs_error_max_pa":max_pressure_error,"height_abs_error_max_all_levels_m":max_height_error_all_levels}});
            crate::state::atomic(output.join("cost.json"), &report)?;
            println!("{}", serde_json::to_string_pretty(&report)?);
            reports.push(report);
        }
        let mean = reports
            .iter()
            .map(|r| r["added_cloud_source_seconds"].as_f64().unwrap())
            .sum::<f64>()
            / reports.len() as f64;
        let summary = json!({"hours":reports,"mean_added_cloud_source_seconds":mean,"scope":"All retained CONUS cells, 50 exact fractions and two exact geometry anchors. Each hour uses its own original cloud snapshot. A weather-only primer equalizes reference preparation; independent Source instances and fresh staging directories are used for the timed runs, with A/B order alternated. No cloud metadata is primed. Streams share at most 16 map jobs on a four-worker runtime. Every weather staging byte, fraction, anchor and derived column is checked after the ingestion timer."});
        crate::state::atomic(output.join("cost.json"), &summary)?;
        println!("{}", serde_json::to_string_pretty(&summary)?);
        Ok(())
    }
}
