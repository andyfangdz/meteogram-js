//! Exact per-field/level/lead Float32 palettes shared by spatial cells.
use crate::state::{fields, read_json};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs::{self, File},
    hash::{BuildHasherDefault, Hasher},
    io::{BufReader, BufWriter, Read, Write},
    os::unix::fs::FileExt,
    path::Path,
};

pub const ENCODING: &str = "float32_dictionary_v1";
const FILE: &str = "values.dict.zst";
// Lookup builders retain hash tables as well as values. Finished codebooks do
// not: uniformly bound their aggregate Float32 payload to 1 GB (decimal)
// across every field, for every model.
pub(crate) const MAX_DICTIONARY_ENTRIES: usize = 250_000_000;
const MAX_DICTIONARY_SLOTS: usize = 65_536;
pub(crate) fn count_entries(total: &mut usize, added: usize) -> Result<()> {
    let next = total
        .checked_add(added)
        .context("Dictionary entry overflow")?;
    anyhow::ensure!(
        next <= MAX_DICTIONARY_ENTRIES,
        "Dictionary exceeds bounded whole-codebook budget"
    );
    *total = next;
    Ok(())
}
#[derive(Default)]
pub struct WordHasher(u64);
impl Hasher for WordHasher {
    fn finish(&self) -> u64 {
        self.0
    }
    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.0 = self.0.wrapping_mul(0x100000001b3) ^ u64::from(*b);
        }
    }
    fn write_u32(&mut self, v: u32) {
        self.0 = u64::from(v).wrapping_mul(0x9e3779b97f4a7c15);
        self.0 ^= self.0 >> 32;
    }
}
type WordMap = HashMap<u32, u32, BuildHasherDefault<WordHasher>>;
#[derive(Default)]
pub struct Palette {
    lookup: WordMap,
    values: Vec<u32>,
}
impl Palette {
    fn index(&mut self, bits: u32) -> Result<u32> {
        anyhow::ensure!(
            f32::from_bits(bits).is_finite(),
            "Non-finite dictionary value"
        );
        if let Some(index) = self.lookup.get(&bits) {
            return Ok(*index);
        }
        let index = u32::try_from(self.values.len())?;
        self.values.push(bits);
        self.lookup.insert(bits, index);
        Ok(index)
    }
}
pub fn encode_spatial(values: Vec<f32>) -> Result<(Vec<f32>, Vec<u32>)> {
    let mut palette = Palette::default();
    let mut codes = Vec::with_capacity(values.len());
    for value in values {
        codes.push(f32::from_bits(palette.index(value.to_bits())?));
    }
    anyhow::ensure!(
        palette.values.len() <= 65536,
        "Spatial palette must fit UInt16"
    );
    let (values, remap) = sorted_palette(&palette.values);
    for code in &mut codes {
        *code = f32::from_bits(remap[code.to_bits() as usize]);
    }
    Ok((codes, values))
}
fn sorted_palette(values: &[u32]) -> (Vec<u32>, Vec<u32>) {
    let mut order: Vec<_> = (0..values.len()).collect();
    order
        .sort_unstable_by(|a, b| f32::from_bits(values[*a]).total_cmp(&f32::from_bits(values[*b])));
    let mut remap = vec![0u32; values.len()];
    let sorted = order
        .into_iter()
        .enumerate()
        .map(|(new, old)| {
            remap[old] = new as u32;
            values[old]
        })
        .collect();
    (sorted, remap)
}
pub struct Dictionary {
    pub fields: Vec<Vec<Vec<u32>>>,
}
pub fn widths(m: &Value) -> Result<Vec<usize>> {
    let nt = m["leads"].as_array().context("Leads")?.len();
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Variables")?.len();
    anyhow::ensure!(nt > 0 && nl > 0 && nf > 0, "Empty dictionary axes");
    let pressure_width = nl.checked_mul(nt).context("Dictionary width overflow")?;
    let widths: Vec<_> = (0..fields(m).len())
        .map(|f| if f < nf { pressure_width } else { nt })
        .collect();
    let slots = widths
        .iter()
        .try_fold(0usize, |sum, n| sum.checked_add(*n))
        .context("Dictionary slot overflow")?;
    anyhow::ensure!(
        slots <= MAX_DICTIONARY_SLOTS,
        "Dictionary exceeds bounded slot budget"
    );
    Ok(widths)
}
pub fn encoded(m: &Value) -> bool {
    m["storage"]["encoding"] == ENCODING
}
pub fn staged(m: &Value) -> bool {
    m["staging"]["encoding"] == ENCODING
}
fn info(m: &Value) -> Option<&Value> {
    if encoded(m) {
        Some(&m["storage"]["dictionary"])
    } else if staged(m) {
        Some(&m["staging"]["dictionary"])
    } else {
        None
    }
}
pub fn file_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 65536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hash.update(&buffer[..n]);
    }
    Ok(format!("{:x}", hash.finalize()))
}
fn u32read(reader: &mut impl Read) -> Result<u32> {
    let mut b = [0u8; 4];
    reader.read_exact(&mut b)?;
    Ok(u32::from_le_bytes(b))
}
impl Dictionary {
    pub fn new(fields: Vec<Vec<Vec<u32>>>) -> Self {
        Self { fields }
    }
    pub fn value(&self, field: usize, slot: usize, index: u32) -> Result<u32> {
        self.fields
            .get(field)
            .and_then(|f| f.get(slot))
            .and_then(|p| p.get(index as usize))
            .copied()
            .context("Dictionary index out of range")
    }
    pub fn limits(&self, field: usize) -> Vec<u32> {
        self.fields[field].iter().map(|p| p.len() as u32).collect()
    }
    pub fn save(&self, dir: &Path, m: &Value) -> Result<Value> {
        self.validate(m)?;
        let tmp = dir.join("values.dict.zst.tmp");
        let mut encoder = zstd::stream::write::Encoder::new(File::create(&tmp)?, 1)?;
        encoder.include_checksum(true)?;
        // Batch the small header/word writes before entering the Zstd encoder.
        let mut encoder = BufWriter::with_capacity(65536, encoder);
        encoder.write_all(b"MGDICT01")?;
        encoder.write_all(&(self.fields.len() as u32).to_le_bytes())?;
        for field in &self.fields {
            encoder.write_all(&(field.len() as u32).to_le_bytes())?;
            for palette in field {
                encoder.write_all(&(palette.len() as u32).to_le_bytes())?;
                for bits in palette {
                    encoder.write_all(&bits.to_le_bytes())?;
                }
            }
        }
        encoder
            .into_inner()
            .map_err(|e| e.into_error())?
            .finish()?
            .sync_all()?;
        fs::rename(tmp, dir.join(FILE))?;
        File::open(dir)?.sync_all()?;
        Ok(
            json!({"file":FILE,"sha256":file_hash(&dir.join(FILE))?,"version":1,"encoded_bytes":fs::metadata(dir.join(FILE))?.len(),"entries":self.fields.iter().flatten().map(Vec::len).sum::<usize>()}),
        )
    }
    fn validate(&self, m: &Value) -> Result<()> {
        anyhow::ensure!(
            m["leads"].as_array().map(Vec::len) == Some(1),
            "A codebook must describe exactly one forecast hour"
        );
        let expected = widths(m)?;
        anyhow::ensure!(
            self.fields.len() == expected.len(),
            "Dictionary field count"
        );
        let nc = m["cells"].as_u64().context("Cells")? as usize;
        let mut entries = 0;
        for (field, width) in self.fields.iter().zip(expected) {
            anyhow::ensure!(field.len() == width, "Dictionary width");
            for palette in field {
                count_entries(&mut entries, palette.len())?;
                anyhow::ensure!(
                    !palette.is_empty()
                        && palette.len() <= nc
                        && palette.len() <= 65536
                        && palette.iter().all(|b| f32::from_bits(*b).is_finite()),
                    "Invalid dictionary palette"
                );
            }
        }
        Ok(())
    }
    pub fn load(dir: &Path, m: &Value) -> Result<Self> {
        Self::load_impl(dir, m, MAX_DICTIONARY_ENTRIES)
    }
    pub fn load_values_budget(dir: &Path, m: &Value, budget: usize) -> Result<Self> {
        Self::load_impl(dir, m, budget)
    }
    pub fn entry_count(&self) -> Result<usize> {
        let mut count = 0;
        for palette in self.fields.iter().flatten() {
            count_entries(&mut count, palette.len())?;
        }
        Ok(count)
    }
    fn load_impl(dir: &Path, m: &Value, budget: usize) -> Result<Self> {
        anyhow::ensure!(
            m["leads"].as_array().map(Vec::len) == Some(1),
            "A codebook must describe exactly one forecast hour"
        );
        let meta = info(m).context("Missing hourly dictionary")?;
        anyhow::ensure!(
            meta["version"] == 1 && meta["file"] == FILE,
            "Unknown dictionary format"
        );
        let path = dir.join(FILE);
        anyhow::ensure!(
            meta["sha256"].as_str() == Some(file_hash(&path)?.as_str()),
            "Dictionary identity mismatch"
        );
        let file = File::open(path)?;
        let mut header = [0u8; 5];
        file.read_exact_at(&mut header, 0)?;
        anyhow::ensure!(
            header[..4] == [0x28, 0xb5, 0x2f, 0xfd] && header[4] & 4 != 0,
            "Dictionary requires a checksummed frame"
        );
        let mut reader = BufReader::new(zstd::stream::read::Decoder::new(file)?);
        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        anyhow::ensure!(&magic == b"MGDICT01", "Dictionary version");
        let expected = widths(m)?;
        anyhow::ensure!(
            u32read(&mut reader)? as usize == expected.len(),
            "Dictionary field count"
        );
        let nc = m["cells"].as_u64().context("Cells")? as usize;
        let mut result = Vec::new();
        let mut entries = 0;
        for width in expected {
            anyhow::ensure!(
                u32read(&mut reader)? as usize == width,
                "Dictionary field width"
            );
            let mut field = Vec::new();
            for _ in 0..width {
                let n = u32read(&mut reader)? as usize;
                count_entries(&mut entries, n)?;
                anyhow::ensure!(entries <= budget, "Run exceeds aggregate codebook budget");
                anyhow::ensure!(n > 0 && n <= nc && n <= 65536, "Dictionary palette/budget");
                let mut palette = Vec::with_capacity(n);
                for _ in 0..n {
                    palette.push(u32read(&mut reader)?);
                }
                field.push(palette);
            }
            result.push(field);
        }
        let mut tail = [0u8; 1];
        anyhow::ensure!(reader.read(&mut tail)? == 0, "Trailing dictionary data");
        anyhow::ensure!(
            meta["entries"].as_u64().is_none_or(|n| n == entries as u64),
            "Dictionary entry count differs"
        );
        let dictionary = Self::new(result);
        dictionary.validate(m)?;
        Ok(dictionary)
    }
}
pub fn copy(source: &Path, target: &Path, m: &Value) -> Result<()> {
    if info(m).is_some() {
        fs::copy(source.join(FILE), target.join(FILE))?;
        File::open(target.join(FILE))?.sync_all()?;
        File::open(target)?.sync_all()?;
    }
    Ok(())
}
pub fn inspect(dir: &Path) -> Result<Value> {
    let m = read_json(dir.join("metadata.json"))?;
    let d = Dictionary::load(dir, &m)?;
    Ok(
        json!({"fields":d.fields.iter().map(|f|json!({"slots":f.len(),"entries":f.iter().map(Vec::len).sum::<usize>(),"max_cardinality":f.iter().map(Vec::len).max()})).collect::<Vec<_>>()}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn compact_book_budget_is_global_and_checked_before_allocation() -> Result<()> {
        let mut entries = 0;
        // The run-wide budget is independent of each UInt16 palette's cardinality.
        count_entries(&mut entries, 32_000_001)?;
        let remaining = MAX_DICTIONARY_ENTRIES - entries;
        count_entries(&mut entries, remaining)?;
        assert!(count_entries(&mut entries, 1).is_err());
        assert_eq!(entries, MAX_DICTIONARY_ENTRIES);
        let mut overflow = usize::MAX;
        assert!(count_entries(&mut overflow, 1).is_err());
        let too_many_slots = json!({"leads":vec![0;65536],"levels":[1000],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
        assert!(widths(&too_many_slots).is_err());
        let temp = tempfile::tempdir()?;
        assert_eq!(MAX_DICTIONARY_ENTRIES * 4, 1_000_000_000);
        for model in ["hrrr", "gfs", "aifs", "rrfs"] {
            let budget = MAX_DICTIONARY_ENTRIES;
            let mut entries = 0;
            count_entries(&mut entries, budget)?;
            assert!(count_entries(&mut entries, 1).is_err());
            let path = temp.path().join(FILE);
            let mut encoder = zstd::stream::write::Encoder::new(File::create(&path)?, 1)?;
            encoder.include_checksum(true)?;
            encoder.write_all(b"MGDICT01")?;
            encoder.write_all(&3u32.to_le_bytes())?;
            encoder.write_all(&1u32.to_le_bytes())?;
            encoder.write_all(&((budget + 1) as u32).to_le_bytes())?;
            encoder.finish()?;
            let m = json!({"model":model,"cells":budget+1,"leads":[0],"levels":[1000],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"],"staging":{"encoding":ENCODING,"dictionary":{"file":FILE,"version":1,"sha256":file_hash(&path)?}}});
            let error = Dictionary::load(temp.path(), &m)
                .err()
                .context("Budget was not rejected")?;
            assert!(error.to_string().contains("whole-codebook budget"));
        }
        Ok(())
    }
    #[test]
    fn sorted_spatial_codes_preserve_zero_signs_subnormals_and_extremes() -> Result<()> {
        let bits = [
            0x7f7fffff, 0x80000000, 1, 0xff7fffff, 0, 0x3f812345, 0x80000000,
        ];
        let (codes, palette) = encode_spatial(bits.into_iter().map(f32::from_bits).collect())?;
        for (bits, code) in bits.iter().zip(codes) {
            assert_eq!(palette[code.to_bits() as usize], *bits);
        }
        assert!(palette.windows(2).all(|v| f32::from_bits(v[0])
            .total_cmp(&f32::from_bits(v[1]))
            .is_lt()));
        assert!(palette.contains(&0));
        assert!(palette.contains(&0x80000000));
        Ok(())
    }
    #[test]
    fn codebook_roundtrip_checks_hash_checksum_shape_and_finite_values() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let d = Dictionary::new(vec![vec![vec![0, 0x80000000, 1, 0x7f7fffff]]; 3]);
        let mut m = json!({"cells":4,"leads":[0],"levels":[1000],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
        let info = d.save(temp.path(), &m)?;
        m["staging"] = json!({"encoding":ENCODING,"dictionary":info});
        assert_eq!(Dictionary::load(temp.path(), &m)?.fields, d.fields);
        let mut wrong = m.clone();
        wrong["staging"]["dictionary"]["sha256"] = json!("wrong");
        assert!(Dictionary::load(temp.path(), &wrong).is_err());
        wrong = m.clone();
        wrong["leads"] = json!([0, 1]);
        assert!(Dictionary::load(temp.path(), &wrong).is_err());
        let path = temp.path().join(FILE);
        let mut bytes = fs::read(&path)?;
        *bytes.last_mut().unwrap() ^= 1;
        fs::write(&path, bytes)?;
        m["staging"]["dictionary"]["sha256"] = json!(file_hash(&path)?);
        assert!(Dictionary::load(temp.path(), &m).is_err());
        let mut encoder = zstd::stream::write::Encoder::new(File::create(&path)?, 1)?;
        encoder.include_checksum(true)?;
        encoder.write_all(b"MGDICT01")?;
        encoder.write_all(&3u32.to_le_bytes())?;
        for _ in 0..3 {
            encoder.write_all(&1u32.to_le_bytes())?;
            encoder.write_all(&1u32.to_le_bytes())?;
            encoder.write_all(&f32::NAN.to_bits().to_le_bytes())?;
        }
        encoder.finish()?;
        m["staging"]["dictionary"]["sha256"] = json!(file_hash(&path)?);
        assert!(Dictionary::load(temp.path(), &m).is_err());
        assert!(
            Dictionary::new(vec![vec![vec![f32::INFINITY.to_bits()]]; 3])
                .save(temp.path(), &m)
                .is_err()
        );
        Ok(())
    }
    #[test]
    fn palettes_preserve_bits_and_reject_nonfinite_and_bad_indices() -> Result<()> {
        let bits = [0u32, 0x80000000, 1, 0x7f7fffff, 0xff7fffff, 0x3f812345];
        let input = bits.iter().copied().cycle().take(30).collect::<Vec<_>>();
        let (codes, values) = encode_spatial(input.iter().copied().map(f32::from_bits).collect())?;
        let dictionary = Dictionary::new(vec![vec![values]]);
        for (code, original) in codes.into_iter().zip(input) {
            assert_eq!(dictionary.value(0, 0, code.to_bits())?, original);
        }
        assert!(dictionary.value(0, 0, u32::MAX).is_err());
        assert!(encode_spatial(vec![f32::NAN]).is_err());
        assert!(encode_spatial(vec![f32::INFINITY]).is_err());
        assert!(encode_spatial((0..65537).map(|v| v as f32).collect()).is_err());
        Ok(())
    }
}
