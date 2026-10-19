//! Source tiles preserve each cell; every UInt128 packs eight UInt16 indices.
//! Its eight little-endian UInt16 indices remain independent logical values;
//! ClickHouse Delta(2) handles spatial compression below this representation.
use crate::{
    db::{varuint, Db},
    state::ident,
};
#[cfg(test)]
use anyhow::Context;
use anyhow::Result;
use serde_json::Value;
use std::io::{Cursor, Read};
pub const LANES: usize = 8;
pub const CELLS: usize = 256;
pub const GROUPS: usize = CELLS / LANES;
pub fn tile_cells(m: &Value) -> Result<usize> {
    anyhow::ensure!(m["storage"]["tile_size"] == CELLS, "Unsupported tile size");
    Ok(CELLS)
}
pub fn tiled(m: &Value) -> bool {
    m["storage"]["layout"] == "cell_tiles_v1"
}
pub fn valid_layout(m: &Value) -> bool {
    tiled(m)
        && m["storage"]["encoding"] == crate::dictionary::ENCODING
        && tile_cells(m).is_ok()
        && m["storage"]["packing"] == "uint16x8_le"
        && m["storage"]["element_type"] == "UInt128"
        && m["storage"]["index_type"] == "UInt16"
        && m["storage"]["axes"] == "level_lead"
        && m["storage"]["partitioned_lead"] == true
        && m["storage"]["lead_hour"]
            .as_u64()
            .is_some_and(|lead| m["leads"] == serde_json::json!([lead]))
}
pub fn code(word: u128, lane: usize) -> u32 {
    ((word >> (lane * 16)) & 65535) as u32
}
pub struct LeadInput<'a> {
    pub profiles: &'a [u16],
    pub surfaces: &'a [u16],
    pub levels: usize,
    pub fields: usize,
    pub cells: usize,
}
pub fn slot_offset(cells: usize, stride: usize, cell: usize, slot: usize) -> usize {
    let block = cell / crate::stage::BLOCK_CELLS;
    let first = block * crate::stage::BLOCK_CELLS;
    let count = crate::stage::BLOCK_CELLS.min(cells - first);
    first * stride + slot * count + cell - first
}
pub fn encode_lead(
    names: &[String],
    widths: &[usize],
    ids: &[u32],
    first: usize,
    input: &LeadInput<'_>,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        cfg!(target_endian = "little")
            && !ids.is_empty()
            && first.is_multiple_of(CELLS)
            && input.cells == ids.len()
            && names.len() == input.fields + 2
            && names.len() == widths.len()
            && input.levels > 0
            && input.fields > 0,
        "Direct packed input dimensions"
    );
    anyhow::ensure!(
        input.profiles.len() >= input.cells * input.levels * input.fields
            && input.surfaces.len() >= input.cells * 2,
        "Direct packed buffers"
    );
    let rows = ids.len().div_ceil(CELLS);
    let mut out = Vec::with_capacity(
        rows * (widths.iter().sum::<usize>() * GROUPS * 16 + names.len() * 8 + 16)
            + ids.len() * 4
            + 4096,
    );
    var(&mut out, names.len() + 3);
    var(&mut out, rows);
    for name in ["revision_id", "tile_id"] {
        text(&mut out, name);
        text(&mut out, "UInt32");
        for row in 0..rows {
            out.extend_from_slice(
                &(if name == "revision_id" {
                    1
                } else {
                    u32::try_from(first / CELLS + row)?
                })
                .to_le_bytes(),
            );
        }
    }
    text(&mut out, "cell_ids");
    text(&mut out, "Array(UInt32)");
    for row in 1..=rows {
        out.extend_from_slice(&(ids.len().min(row * CELLS) as u64).to_le_bytes());
    }
    for id in ids {
        out.extend_from_slice(&id.to_le_bytes());
    }
    for (field, name) in names.iter().enumerate() {
        let pressure = field < input.fields;
        let levels = if pressure { input.levels } else { 1 };
        let stride = if pressure {
            input.levels * input.fields
        } else {
            2
        };
        let values = if pressure {
            input.profiles
        } else {
            input.surfaces
        };
        let width = levels;
        anyhow::ensure!(width == widths[field], "Direct packed slot shape");
        text(&mut out, name);
        text(&mut out, "Array(UInt128)");
        for row in 1..=rows {
            out.extend_from_slice(&((row * width * GROUPS) as u64).to_le_bytes());
        }
        let data = out.len();
        out.resize(data + rows * width * GROUPS * 16, 0);
        let full_rows = input.cells / CELLS;
        // Group sixteen tiles, keeping adjacent output cache lines hot.
        // Full words use a fixed-size copy; only the final short tile pads.
        for first_row in (0..full_rows).step_by(16) {
            let group_rows = 16.min(full_rows - first_row);
            let cell = first_row * CELLS;
            for level in 0..levels {
                let source_slot = if pressure {
                    level * input.fields + field
                } else {
                    field - input.fields
                };
                let source = slot_offset(input.cells, stride, cell, source_slot);
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        values[source..source + group_rows * CELLS]
                            .as_ptr()
                            .cast::<u8>(),
                        group_rows * CELLS * 2,
                    )
                };
                for row in 0..group_rows {
                    let target = data + ((first_row + row) * width + level) * GROUPS * 16;
                    out[target..target + CELLS * 2]
                        .copy_from_slice(&bytes[row * CELLS * 2..(row + 1) * CELLS * 2]);
                }
            }
        }
        if !input.cells.is_multiple_of(CELLS) {
            let cell = full_rows * CELLS;
            let take = input.cells - cell;
            for level in 0..levels {
                let source_slot = if pressure {
                    level * input.fields + field
                } else {
                    field - input.fields
                };
                let source = slot_offset(input.cells, stride, cell, source_slot);
                let target = data + (full_rows * width + level) * GROUPS * 16;
                let bytes = unsafe {
                    std::slice::from_raw_parts(
                        values[source..source + take].as_ptr().cast::<u8>(),
                        take * 2,
                    )
                };
                out[target..target + bytes.len()].copy_from_slice(bytes);
            }
        }
    }
    Ok(out)
}
pub fn array_check(name: &str, width: usize, limits: &[u32], groups: usize) -> String {
    let width = width * groups;
    let bounds = limits
        .iter()
        .map(u32::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let bad = if limits.iter().all(|n| *n <= 32768) {
        // Every lane has a guard high bit, so subtracting the broadcast bound
        // cannot borrow across lanes. A high result bit marks code >= bound.
        let ones = (0..8).map(|i| 1u128 << (16 * i)).sum::<u128>();
        let high = ones * 32768;
        format!("bitAnd(bitOr(bitOr(word,toUInt128('{high}'))-bound,word),toUInt128('{high}'))!=0")
    } else {
        // The complete UInt16 domain is supported, including bound=65536.
        let codes = (0..8)
            .map(|i| format!("toUInt16(bitShiftRight(word,{}))", i * 16))
            .collect::<Vec<_>>()
            .join(",");
        format!("greatest({codes})>=bound")
    };
    let bounds = if limits.iter().all(|n| *n <= 32768) {
        let ones = (0..8).map(|i| 1u128 << (16 * i)).sum::<u128>();
        let bytes: Vec<_> = limits
            .iter()
            .flat_map(|n| (u128::from(*n) * ones).to_le_bytes())
            .collect();
        // Compact binary constants avoid both per-word multiplication and the
        // SQL parser's 256 KiB limit for decimal UInt128 arrays. The slot bounds
        // still come directly from the hash-checked, finite codebook.
        format!(
            "reinterpret(base64Decode('{}'),'Array(UInt128)')",
            base64(&bytes)
        )
    } else {
        format!("CAST([{bounds}] AS Array(UInt32))")
    };
    // This is a constant expression: broadcast each logical slot's exact bound
    // once per word group without repeating the whole binary literal in SQL.
    let bounds =
        format!("arrayFlatten(arrayMap(bound -> arrayWithConstant({groups},bound),{bounds}))");
    format!("length({name})!={width} OR arrayExists((word,bound)->({bad}),{name},{bounds})")
}
fn base64(bytes: &[u8]) -> String {
    const LETTERS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let word = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(LETTERS[(word >> 18) as usize] as char);
        out.push(LETTERS[((word >> 12) & 63) as usize] as char);
        out.push(if chunk.len() > 1 {
            LETTERS[((word >> 6) & 63) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            LETTERS[(word & 63) as usize] as char
        } else {
            '='
        });
    }
    out
}
#[cfg(test)]
pub fn verify_cells(
    db: &Db,
    database: &str,
    table: &str,
    ids: &[u32],
    tile_cells: usize,
) -> Result<()> {
    verify_cells_scoped(db, database, table, ids, tile_cells, None)
}
pub fn verify_cells_scoped(
    db: &Db,
    database: &str,
    table: &str,
    ids: &[u32],
    tile_cells: usize,
    lead: Option<u16>,
) -> Result<()> {
    ident(database)?;
    ident(table)?;
    let bytes = db.bytes(&format!(
        "SELECT tile_id,cell_ids FROM {database}.{table} {} ORDER BY tile_id FORMAT RowBinary",
        lead.map(|h| format!("WHERE lead_hour={h}"))
            .unwrap_or_default()
    ))?;
    let mut cursor = Cursor::new(bytes.as_slice());
    for (tile, expected) in ids.chunks(tile_cells).enumerate() {
        let mut b = [0u8; 4];
        cursor.read_exact(&mut b)?;
        anyhow::ensure!(
            u32::from_le_bytes(b) == tile as u32,
            "Missing/duplicate/substituted tile key"
        );
        anyhow::ensure!(
            varuint(&mut cursor)? == expected.len(),
            "Incorrect tile cell count"
        );
        for id in expected {
            cursor.read_exact(&mut b)?;
            anyhow::ensure!(
                u32::from_le_bytes(b) == *id,
                "Incorrect ordered tile cell IDs"
            );
        }
    }
    anyhow::ensure!(
        cursor.position() == bytes.len() as u64,
        "Unexpected tile rows"
    );
    Ok(())
}
pub fn append_lead_column(mut native: Vec<u8>, rows: usize, lead: u16) -> Result<Vec<u8>> {
    anyhow::ensure!(
        native.first().is_some_and(|n| *n < 127),
        "Unexpected Native header"
    );
    native[0] += 1;
    text(&mut native, "lead_hour");
    text(&mut native, "UInt16");
    for _ in 0..rows {
        native.extend_from_slice(&lead.to_le_bytes());
    }
    Ok(native)
}
fn var(out: &mut Vec<u8>, mut n: usize) {
    while n >= 128 {
        out.push((n as u8 & 127) | 128);
        n >>= 7;
    }
    out.push(n as u8);
}
fn text(out: &mut Vec<u8>, s: &str) {
    var(out, s.len());
    out.extend_from_slice(s.as_bytes());
}
#[cfg(test)]
fn reference_words(values: &[f32], width: usize, cells: usize, out: &mut [u8]) -> Result<()> {
    anyhow::ensure!(
        values.len() == width * cells && out.len() == cells.div_ceil(CELLS) * width * GROUPS * 16,
        "Packed buffer dimensions"
    );
    for tile in 0..cells.div_ceil(CELLS) {
        for slot in 0..width {
            for group in 0..GROUPS {
                let mut word = 0u128;
                for lane in 0..LANES {
                    let cell = tile * CELLS + group * LANES + lane;
                    let index = if cell < cells {
                        u16::try_from(values[cell * width + slot].to_bits())
                            .context("UInt16 packed index overflow")?
                    } else {
                        0
                    };
                    word |= u128::from(index) << (lane * 16);
                }
                let at = ((tile * width + slot) * GROUPS + group) * 16;
                out[at..at + 16].copy_from_slice(&word.to_le_bytes());
            }
        }
    }
    Ok(())
}
#[cfg(test)]
fn reference_block(
    names: &[String],
    columns: &[&[f32]],
    widths: &[usize],
    ids: &[u32],
    first: usize,
) -> Result<Vec<u8>> {
    anyhow::ensure!(
        cfg!(target_endian = "little")
            && !ids.is_empty()
            && first.is_multiple_of(CELLS)
            && names.len() == columns.len()
            && names.len() == widths.len(),
        "Packed batch dimensions"
    );
    let rows = ids.len().div_ceil(CELLS);
    let mut out = Vec::with_capacity(
        rows * (widths.iter().sum::<usize>() * GROUPS * 16 + names.len() * 8 + 8)
            + ids.len() * 4
            + rows * 8
            + 4096,
    );
    var(&mut out, names.len() + 3);
    var(&mut out, rows);
    for name in ["revision_id", "tile_id"] {
        text(&mut out, name);
        text(&mut out, "UInt32");
        for row in 0..rows {
            out.extend_from_slice(
                &(if name == "revision_id" {
                    1
                } else {
                    u32::try_from(first / CELLS + row)?
                })
                .to_le_bytes(),
            );
        }
    }
    text(&mut out, "cell_ids");
    text(&mut out, "Array(UInt32)");
    for row in 1..=rows {
        out.extend_from_slice(&(ids.len().min(row * CELLS) as u64).to_le_bytes());
    }
    for id in ids {
        out.extend_from_slice(&id.to_le_bytes());
    }
    for (f, name) in names.iter().enumerate() {
        text(&mut out, name);
        text(&mut out, "Array(UInt128)");
        let width = widths[f];
        for row in 1..=rows {
            out.extend_from_slice(&((row * width * GROUPS) as u64).to_le_bytes());
        }
        let start = out.len();
        out.resize(start + rows * width * GROUPS * 16, 0);
        reference_words(columns[f], width, ids.len(), &mut out[start..])?;
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn direct_stage_tiles_match_normalized_native_bytes_across_frame_and_cell_tails() -> Result<()>
    {
        let root = tempfile::tempdir()?;
        let nt = 1;
        let nl = 2;
        let nf = 2;
        let names: Vec<_> = ["p0", "p1", "s0", "s1"]
            .into_iter()
            .map(str::to_owned)
            .collect();
        let widths = [nl * nt, nl * nt, nt, nt];
        for nc in [7usize, 8, 9, 4101, 8197] {
            let mut profiles = Vec::new();
            let mut surfaces = Vec::new();
            let mut original_p = Vec::new();
            let mut original_s = Vec::new();
            for t in 0..nt {
                let p: Vec<_> = (0..nc * nl * nf)
                    .map(|i| {
                        f32::from_bits(
                            (i as u32)
                                .wrapping_mul(2654435761)
                                .wrapping_add(t as u32 * 23)
                                & 65535,
                        )
                    })
                    .collect();
                let s: Vec<_> = (0..nc * 2)
                    .map(|i| {
                        f32::from_bits(
                            (i as u32).wrapping_mul(197).wrapping_add(t as u32 * 17) & 65535,
                        )
                    })
                    .collect();
                let pp = root.path().join(format!("p-{nc}-{t}"));
                let sp = root.path().join(format!("s-{nc}-{t}"));
                crate::stage::write_codes(&pp, &[nc, nl, nf], &p)?;
                crate::stage::write_codes(&sp, &[nc, 2], &s)?;
                profiles.push(crate::stage::Array::open_as(
                    &pp,
                    &[nc, nl, nf],
                    "uint16_le",
                )?);
                surfaces.push(crate::stage::Array::open_as(&sp, &[nc, 2], "uint16_le")?);
                original_p.push(p);
                original_s.push(s);
            }
            for first in if nc > 4096 { vec![0, 4096] } else { vec![0] } {
                let cells = nc - first;
                let mut p = vec![vec![0u16; cells * nl * nf]; nt];
                let mut s = vec![vec![0u16; cells * 2]; nt];
                let mut scratch = crate::stage::Scratch::new()?;
                for t in 0..nt {
                    profiles[t].read_codes_aligned(first, cells, &mut p[t], &mut scratch)?;
                    surfaces[t].read_codes_aligned(first, cells, &mut s[t], &mut scratch)?;
                }
                let ids: Vec<_> = (first..nc).map(|i| i as u32 * 11 + 7).collect();
                let mut canonical: Vec<_> = widths.iter().map(|w| vec![0f32; cells * w]).collect();
                for cell in 0..cells {
                    for t in 0..nt {
                        for level in 0..nl {
                            for field in 0..nf {
                                canonical[field][(cell * nl + level) * nt + t] =
                                    original_p[t][((first + cell) * nl + level) * nf + field];
                            }
                        }
                        for field in 0..2 {
                            canonical[nf + field][cell * nt + t] =
                                original_s[t][(first + cell) * 2 + field];
                        }
                    }
                }
                let columns: Vec<_> = canonical.iter().map(Vec::as_slice).collect();
                let reference = reference_block(&names, &columns, &widths, &ids, first)?;
                let direct = encode_lead(
                    &names,
                    &widths,
                    &ids,
                    first,
                    &LeadInput {
                        profiles: &p[0],
                        surfaces: &s[0],
                        levels: nl,
                        fields: nf,
                        cells,
                    },
                )?;
                assert_eq!(direct, reference);
            }
        }
        Ok(())
    }
    #[test]
    #[ignore = "requires the isolated loopback ClickHouse server"]
    fn database_packed_validation_covers_every_u16_value_boundaries_and_id_order() {
        let db = Db::new().unwrap();
        let table = format!("rust_test_packed_{}", uuid::Uuid::new_v4().simple());
        db.command(&format!("CREATE TABLE meteogram_native.{table} (revision_id UInt32,tile_id UInt32,cell_ids Array(UInt32),value Array(UInt128) CODEC(Delta(2),ZSTD(1))) ENGINE=MergeTree ORDER BY tile_id SETTINGS index_granularity=1")).unwrap();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ids: Vec<_> = (0..65536).collect();
            let values: Vec<_> = ids.iter().map(|i| f32::from_bits(*i)).collect();
            db.insert(
                &table,
                reference_block(&["value".to_owned()], &[&values], &[1], &ids, 0).unwrap(),
            )
            .unwrap();
            verify_cells(&db, "meteogram_native", &table, &ids, CELLS).unwrap();
            for bound in [1u32, 2, 7, 8, 9, 32767, 32768, 32769, 65535, 65536] {
                let check = array_check("value", 1, &[bound], GROUPS);
                assert_eq!(db.scalar(&format!("SELECT count() FROM meteogram_native.{table} WHERE {check} SETTINGS compile_expressions=1,min_count_to_compile_expression=0")).unwrap(),(65536/CELLS as u32-bound/CELLS as u32) as u64);
            }
            db.command(&format!("TRUNCATE TABLE meteogram_native.{table}"))
                .unwrap();
            for bound in [1u32, 32767, 32768, 32769, 65535, 65536] {
                db.command(&format!("TRUNCATE TABLE meteogram_native.{table}"))
                    .unwrap();
                let good = (0..8)
                    .map(|lane| u128::from(bound - 1) << (lane * 16))
                    .sum::<u128>();
                let array = |words: &[u128]| {
                    words
                        .iter()
                        .map(|v| format!("toUInt128('{v}')"))
                        .collect::<Vec<_>>()
                        .join(",")
                };
                let mut rows = vec![format!("(1,0,[0],[{}])", array(&[good; GROUPS]))];
                if bound < 65536 {
                    for cell in 0..CELLS {
                        let lane = cell % LANES;
                        let word = (good & !(65535u128 << (lane * 16)))
                            | (u128::from(bound) << (lane * 16));
                        let mut words = vec![good; GROUPS];
                        words[cell / LANES] = word;
                        rows.push(format!("(1,{},[0],[{}])", cell + 1, array(&words)));
                    }
                }
                db.command(&format!(
                    "INSERT INTO meteogram_native.{table} VALUES {}",
                    rows.join(",")
                ))
                .unwrap();
                assert_eq!(db.scalar(&format!("SELECT count() FROM meteogram_native.{table} WHERE {} SETTINGS compile_expressions=1,min_count_to_compile_expression=0",array_check("value",1,&[bound],GROUPS))).unwrap(),if bound<65536 {CELLS as u64}else{0});
            }
            db.command(&format!("TRUNCATE TABLE meteogram_native.{table}"))
                .unwrap();
            db.insert(
                &table,
                reference_block(&["value".to_owned()], &[&values], &[1], &ids, 0).unwrap(),
            )
            .unwrap();
            db.command(&format!("ALTER TABLE meteogram_native.{table} UPDATE cell_ids=arrayReverse(cell_ids) WHERE tile_id=0 SETTINGS mutations_sync=2")).unwrap();
            assert!(verify_cells(&db, "meteogram_native", &table, &ids, CELLS).is_err());
        }));
        let _ = db.command(&format!(
            "DROP TABLE IF EXISTS meteogram_native.{table} SYNC"
        ));
        assert!(result.is_ok());
    }
    #[test]
    fn packed_words_preserve_all_lanes_slots_tails_and_reject_overflow() -> Result<()> {
        for (cells, width) in [
            (1usize, 1usize),
            (7, 3),
            (8, 4),
            (9, 7),
            (8197, 49),
            (17, 1470),
        ] {
            let input: Vec<_> = (0..cells * width)
                .map(|i| f32::from_bits((i as u32).wrapping_mul(2654435761) & 65535))
                .collect();
            let mut out = vec![0u8; cells.div_ceil(CELLS) * width * GROUPS * 16];
            reference_words(&input, width, cells, &mut out)?;
            for (tile, slots) in out.chunks_exact(width * GROUPS * 16).enumerate() {
                for (word_index, bytes) in slots.as_chunks::<16>().0.iter().enumerate() {
                    let slot = word_index / GROUPS;
                    let group = word_index % GROUPS;
                    let word = u128::from_le_bytes(*bytes);
                    for lane in 0..LANES {
                        let cell = tile * CELLS + group * LANES + lane;
                        assert_eq!(
                            code(word, lane),
                            if cell < cells {
                                input[cell * width + slot].to_bits()
                            } else {
                                0
                            }
                        );
                    }
                }
            }
        }
        for lane in 0..8 {
            let mut input = vec![f32::from_bits(65535); 8 * 4];
            input[lane * 4] = f32::from_bits(65536);
            assert!(reference_words(&input, 4, 8, &mut vec![0u8; 4 * GROUPS * 16]).is_err());
        }
        Ok(())
    }
}
