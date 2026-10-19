//! Bounded offline preparation from an immutable full-profile source table.
//! This is source preparation, never part of the timed loader benchmark.
use crate::{
    db::{varuint, Db},
    dictionary::{Dictionary, FieldBuilder, ENCODING},
    stage::{self, BlockWriter},
    state::*,
};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    io::{Cursor, Read},
    path::Path,
    sync::atomic::{AtomicUsize, Ordering},
    time::Instant,
};

/// Unlike FieldBuilder, this retains only one spatial map's lookup table.
/// Raw input and output matrices are each limited to 1 GiB, independently of
/// the uniform 1 GB compact codebook limit. No forecast axis or native cell is cut.
pub fn encode_stage(source: &Path, out: &Path) -> Result<Value> {
    anyhow::ensure!(!out.exists(), "Preparation needs a fresh output directory");
    let started = Instant::now();
    let mut m = read_json(source.join("metadata.json"))?;
    anyhow::ensure!(
        !crate::dictionary::staged(&m) && !crate::dictionary::encoded(&m),
        "Mapwise encoding requires raw Float32 staging"
    );
    let nc = usize::try_from(m["cells"].as_u64().context("Cells")?)?;
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let leads = m["leads"].as_array().context("Leads")?.clone();
    let nt = leads.len();
    anyhow::ensure!(
        nc > 0
            && nc <= 2_000_000
            && nl > 0
            && nf > 0
            && nt > 0
            && m["surface_variables"].as_array().map(Vec::len) == Some(2),
        "Mapwise staging axes/cell budget"
    );
    let stride = nl.checked_mul(nf).context("Staging stride overflow")?;
    let matrix_bytes = nc
        .checked_mul(stride.max(2))
        .and_then(|n| n.checked_mul(4))
        .context("Staging matrix overflow")?;
    anyhow::ensure!(
        matrix_bytes <= 1 << 30,
        "Mapwise staging matrix exceeds 1 GiB budget"
    );
    let cells = stage::load_cells(source)?;
    let ids = &cells["cell_id"].bytes;
    anyhow::ensure!(
        ids.len() == nc * 4
            && ids
                .as_chunks::<4>()
                .0
                .windows(2)
                .all(|p| u32::from_le_bytes(p[0]) < u32::from_le_bytes(p[1])),
        "Source cell order/coverage"
    );
    let mut palettes: Vec<Vec<Vec<u32>>> = crate::dictionary::widths(&m)?
        .into_iter()
        .map(|width| vec![Vec::new(); width])
        .collect();
    fs::create_dir_all(out)?;
    stage::copy_cells(source, out)?;
    m.as_object_mut()
        .context("Metadata object")?
        .remove("storage");
    m["staging"] =
        json!({"encoding":ENCODING,"palette_order":"float32_total_order","complete":false});
    atomic(out.join("metadata.json"), &m)?;
    let mut entries = 0;
    let mut encoded_bytes = 0u64;
    let mut max_palette = 0;
    for (t, lead) in leads.iter().enumerate() {
        disk_guard(out, 0)?;
        let lead = lead.as_u64().context("Invalid lead")?;
        for (prefix, shape, width) in [
            ("lead", vec![nc, nl, nf], stride),
            ("surface", vec![nc, 2], 2),
        ] {
            let name = format!("{prefix}-{lead:03}.npy");
            let input = stage::Array::open_as(&source.join(&name), &shape, "float32_le")?;
            let mut values = vec![0f32; nc * width];
            input.read_into(0, &mut values)?;
            let mut codes = vec![0u32; values.len()];
            for slot in 0..width {
                let spatial = (0..nc).map(|cell| values[cell * width + slot]).collect();
                let (map_codes, palette) = crate::dictionary::encode_spatial(spatial)?;
                anyhow::ensure!(
                    palette.len() <= 65536,
                    "Spatial palette exceeds UInt16; retain raw storage"
                );
                crate::dictionary::count_entries(&mut entries, palette.len())?;
                max_palette = max_palette.max(palette.len());
                for (cell, code) in map_codes.into_iter().enumerate() {
                    codes[cell * width + slot] = code.to_bits();
                }
                let (f, position) = if prefix == "lead" {
                    (slot % nf, (slot / nf) * nt + t)
                } else {
                    (nf + slot, t)
                };
                palettes[f][position] = palette;
            }
            drop(values);
            let mut writer = BlockWriter::new(&out.join(&name), &shape, "uint16_le")?;
            for chunk in codes.chunks(stage::BLOCK_CELLS * width) {
                writer.append(chunk)?;
            }
            encoded_bytes += writer.finish()?["encoded_bytes"]
                .as_u64()
                .context("Stage bytes")?;
        }
        println!(
            "{}",
            json!({"phase":"mapwise_dictionary","lead":lead,"leads_complete":t+1,"leads":nt,"entries":entries,"seconds":started.elapsed().as_secs_f64()})
        );
    }
    let info = Dictionary::new(palettes).save(out, &m)?;
    m["staging"]["dictionary"] = info.clone();
    m["staging"]["complete"] = json!(true);
    atomic(out.join("metadata.json"), &m)?;
    let report = json!({"cells":nc,"leads":nt,"levels":nl,"fields":nf,"logical_rows":nc*nt,"dictionary":info,"coded_stage_bytes":encoded_bytes,"index_type":"UInt16","max_palette_entries":max_palette,"whole_codebook_entry_budget":crate::dictionary::MAX_DICTIONARY_ENTRIES,"matrix_bytes":matrix_bytes,"max_matrix_bytes":1u64<<30,"max_cells":2_000_000,"lookup_lifetime":"one_spatial_map","seconds":started.elapsed().as_secs_f64(),"source_preparation_not_loader_timing":true});
    atomic(out.join("preparation.json"), &report)?;
    Ok(report)
}

pub fn sorted_stage(source: &Path, out: &Path) -> Result<Value> {
    anyhow::ensure!(!out.exists(), "Preparation needs a fresh output directory");
    let started = Instant::now();
    let mut m = read_json(source.join("metadata.json"))?;
    let original = Dictionary::load(source, &m)?.context("No source dictionary")?;
    let (dictionary, remaps) = original.sorted();
    // Verify every palette entry's bijection before reading any spatial codes.
    for (f, field) in original.fields.iter().enumerate() {
        for (slot, palette) in field.iter().enumerate() {
            for (code, bits) in palette.iter().enumerate() {
                anyhow::ensure!(
                    dictionary.value(f, slot, remaps[f][slot][code])? == *bits,
                    "Non-exact palette remap"
                );
            }
        }
    }
    drop(original);
    let nc = usize::try_from(m["cells"].as_u64().context("Cells")?)?;
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let leads = m["leads"].as_array().context("Leads")?.clone();
    let nt = leads.len();
    let short = dictionary.fields.iter().flatten().all(|p| p.len() <= 65536);
    let dtype = if short { "uint16_le" } else { "uint32_le" };
    fs::create_dir_all(out)?;
    stage::copy_cells(source, out)?;
    let info = dictionary.save(out, &m)?;
    m.as_object_mut()
        .context("Metadata object")?
        .remove("storage");
    m["staging"] = json!({"encoding":ENCODING,"dictionary":info,"palette_order":"float32_total_order","complete":false});
    atomic(out.join("metadata.json"), &m)?;
    let next = AtomicUsize::new(0);
    let bytes = std::thread::scope(|scope| -> Result<u64> {
        let mut handles = Vec::new();
        for _ in 0..8.min(nt) {
            let (next, leads, remaps) = (&next, &leads, &remaps);
            handles.push(scope.spawn(move || -> Result<u64> {
                let mut encoded_bytes = 0;
                loop {
                    let t = next.fetch_add(1, Ordering::Relaxed);
                    if t >= nt { break; }
                    disk_guard(out, 0)?;
                    let lead = leads[t].as_u64().context("Lead")?;
                    for (prefix, shape, stride) in [("lead",vec![nc,nl,nf],nl*nf),("surface",vec![nc,2],2)] {
                        let name = format!("{prefix}-{lead:03}.npy");
                        let input = stage::Array::open_as(&source.join(&name), &shape, "uint32_le")?;
                        let mut scratch = stage::Scratch::new()?;
                        let mut writer = BlockWriter::new(&out.join(&name), &shape, dtype)?;
                        let mut buffer = vec![0f32;4096*stride];
                        for start in (0..nc).step_by(4096) {
                            let cells = 4096.min(nc-start);
                            let values = &mut buffer[..cells*stride];
                            input.read_into_with(start*stride, values, &mut scratch)?;
                            for cell in values.chunks_exact_mut(stride) {
                                for (i, code) in cell.iter_mut().enumerate() {
                                    let (f,slot) = if prefix == "lead" { (i%nf,(i/nf)*nt+t) } else { (nf+i,t) };
                                    let remap = remaps[f][slot].get(code.to_bits() as usize).context("Invalid source dictionary code")?;
                                    *code = f32::from_bits(*remap);
                                }
                            }
                            let words = unsafe { std::slice::from_raw_parts(values.as_ptr().cast::<u32>(),values.len()) };
                            writer.append(words)?;
                        }
                        encoded_bytes += writer.finish()?["encoded_bytes"].as_u64().context("Stage bytes")?;
                    }
                    println!("{}",json!({"phase":"sorted_stage","lead":lead,"seconds":started.elapsed().as_secs_f64()}));
                }
                Ok(encoded_bytes)
            }));
        }
        let mut total = 0;
        let mut error = None;
        for handle in handles {
            match handle
                .join()
                .unwrap_or_else(|_| Err(anyhow::anyhow!("Preparation worker panicked")))
            {
                Ok(bytes) => total += bytes,
                Err(e) => {
                    error.get_or_insert(e);
                }
            }
        }
        if let Some(e) = error {
            return Err(e);
        }
        Ok(total)
    })?;
    m["staging"]["complete"] = json!(true);
    atomic(out.join("metadata.json"), &m)?;
    let report = json!({"cells":nc,"leads":nt,"levels":nl,"coded_stage_bytes":bytes,"index_type":if short { "UInt16" } else { "UInt32" },"dictionary":info,"seconds":started.elapsed().as_secs_f64(),"source_preparation_not_loader_timing":true});
    atomic(out.join("preparation.json"), &report)?;
    Ok(report)
}
pub fn from_store(
    source: &Path,
    database: &str,
    table: &str,
    out: &Path,
    raw: Option<&Path>,
) -> Result<Value> {
    from_store_with_mode(source, database, table, out, raw, false)
}
pub fn from_store_with_mode(
    source: &Path,
    database: &str,
    table: &str,
    out: &Path,
    raw: Option<&Path>,
    raw_only: bool,
) -> Result<Value> {
    anyhow::ensure!(!raw_only || raw.is_none(), "Raw-only export has one output");
    ident(database)?;
    ident(table)?;
    anyhow::ensure!(
        !out.exists() && raw.is_none_or(|p| !p.exists()),
        "Preparation needs fresh output directories"
    );
    let original = read_json(source.join("metadata.json"))?;
    let mut m = original.clone();
    anyhow::ensure!(
        (m["storage"]["layout"] == "cell_profile_v1" || crate::packed::valid_layout(&m))
            && m["storage"]["axes"] == "level_lead",
        "Preparation needs complete native cell profiles"
    );
    let source_dictionary = Dictionary::load(source, &m)?;
    let source_value_bytes = crate::dictionary::index_bytes(&m)?;
    let cells = stage::load_cells(source)?;
    let ids: Vec<_> = cells["cell_id"]
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect();
    let nc = ids.len();
    anyhow::ensure!(
        nc > 0
            && nc as u64 == m["cells"].as_u64().context("Cells")?
            && ids.windows(2).all(|p| p[0] < p[1]),
        "Cell index"
    );
    let nl = m["levels"].as_array().context("Levels")?.len();
    let nf = m["variables"].as_array().context("Fields")?.len();
    let leads = m["leads"].as_array().context("Leads")?.clone();
    let nt = leads.len();
    let widths = crate::dictionary::widths(&m)?;
    let names = fields(&m);
    for f in &names {
        ident(f)?;
    }
    fs::create_dir_all(out)?;
    stage::copy_cells(source, out)?;
    if let Some(raw) = raw {
        fs::create_dir_all(raw)?;
        stage::copy_cells(source, raw)?;
    }
    let writers = |dir: &Path, dtype: &str| -> Result<Vec<(BlockWriter, BlockWriter)>> {
        leads
            .iter()
            .map(|lead| {
                let lead = lead.as_u64().context("Lead")?;
                Ok((
                    BlockWriter::new(
                        &dir.join(format!("lead-{lead:03}.npy")),
                        &[nc, nl, nf],
                        dtype,
                    )?,
                    BlockWriter::new(&dir.join(format!("surface-{lead:03}.npy")), &[nc, 2], dtype)?,
                ))
            })
            .collect()
    };
    let mut coded = if raw_only {
        None
    } else {
        Some(writers(out, "uint32_le")?)
    };
    let mut raw_writers = if raw_only {
        Some(writers(out, "float32_le")?)
    } else {
        raw.map(|p| writers(p, "float32_le")).transpose()?
    };
    let mut builders: Vec<_> = if raw_only {
        Vec::new()
    } else {
        widths.iter().map(|w| FieldBuilder::new(*w)).collect()
    };
    let db = Db::new()?;
    let started = Instant::now();
    let mut read_seconds = 0.;
    let mut encode_seconds = 0.;
    let mut stage_seconds = 0.;
    for start in (0..nc).step_by(4096) {
        disk_guard(out, 0)?;
        let count = 4096.min(nc - start);
        let tick = Instant::now();
        let columns = if crate::packed::tiled(&m) {
            crate::packed::export_range(
                &db,
                database,
                table,
                &ids,
                &m,
                source_dictionary
                    .as_ref()
                    .context("Missing packed source dictionary")?,
                start..start + count,
            )?
        } else {
            let bytes=db.bytes(&format!("SELECT cell_id,{} FROM {database}.{table} WHERE cell_id>={} AND cell_id<={} ORDER BY cell_id SETTINGS max_threads=2,max_block_size=1024 FORMAT RowBinary",names.join(","),ids[start],ids[start+count-1]))?;
            let mut columns: Vec<_> = widths
                .iter()
                .map(|w| Vec::with_capacity(count * w))
                .collect();
            let mut cursor = Cursor::new(bytes.as_slice());
            for &id in &ids[start..start + count] {
                let mut b = [0u8; 4];
                cursor.read_exact(&mut b)?;
                anyhow::ensure!(u32::from_le_bytes(b) == id, "Source cell order/coverage");
                for (f, width) in widths.iter().enumerate() {
                    anyhow::ensure!(varuint(&mut cursor)? == *width, "Source profile shape");
                    for slot in 0..*width {
                        b.fill(0);
                        cursor.read_exact(&mut b[..source_value_bytes])?;
                        let value = u32::from_le_bytes(b);
                        columns[f].push(if let Some(dictionary) = &source_dictionary {
                            dictionary.value(f, slot, value)?
                        } else {
                            value
                        });
                    }
                }
            }
            anyhow::ensure!(
                cursor.position() == bytes.len() as u64,
                "Unexpected source rows"
            );
            columns
        };
        read_seconds += tick.elapsed().as_secs_f64();
        let tick = Instant::now();
        if raw_only {
            anyhow::ensure!(
                columns
                    .iter()
                    .flatten()
                    .all(|bits| f32::from_bits(*bits).is_finite()),
                "Non-finite raw export"
            );
        }
        let indices = if raw_only {
            None
        } else {
            Some(std::thread::scope(|scope| -> Result<Vec<Vec<u32>>> {
                let jobs: Vec<_> = builders
                    .iter_mut()
                    .zip(&columns)
                    .map(|(builder, values)| scope.spawn(move || builder.encode(values)))
                    .collect();
                jobs.into_iter()
                    .map(|j| {
                        j.join().map_err(|_| {
                            anyhow::anyhow!("Dictionary preparation worker panicked")
                        })?
                    })
                    .collect()
            })?)
        };
        encode_seconds += tick.elapsed().as_secs_f64();
        let tick = Instant::now();
        fn emit(
            columns: &[Vec<u32>],
            writers: &mut [(BlockWriter, BlockWriter)],
            count: usize,
            nl: usize,
            nf: usize,
            nt: usize,
        ) -> Result<()> {
            let mut profiles: Vec<_> = (0..nt).map(|_| vec![0u32; count * nl * nf]).collect();
            let mut ground: Vec<_> = (0..nt).map(|_| vec![0u32; count * 2]).collect();
            for base in (0..count).step_by(16) {
                for t in 0..nt {
                    for c in base..(base + 16).min(count) {
                        for l in 0..nl {
                            for f in 0..nf {
                                profiles[t][(c * nl + l) * nf + f] =
                                    columns[f][(c * nl + l) * nt + t];
                            }
                        }
                        for f in 0..2 {
                            ground[t][c * 2 + f] = columns[nf + f][c * nt + t];
                        }
                    }
                }
            }
            for (t, (p, g)) in writers.iter_mut().enumerate() {
                p.append(&profiles[t])?;
                g.append(&ground[t])?;
            }
            Ok(())
        }
        if let (Some(indices), Some(coded)) = (&indices, &mut coded) {
            emit(indices, coded, count, nl, nf, nt)?;
        }
        if let Some(writers) = &mut raw_writers {
            emit(&columns, writers, count, nl, nf, nt)?;
        }
        stage_seconds += tick.elapsed().as_secs_f64();
        if start % (4096 * 10) == 0 || start + count == nc {
            println!(
                "{}",
                json!({"phase":"prepare_dictionary","cells":start+count,"total":nc,"seconds":started.elapsed().as_secs_f64()})
            );
        }
    }
    let info = if raw_only {
        None
    } else {
        let entries = builders.into_iter().map(FieldBuilder::finish).collect();
        Some(Dictionary::new(entries).save(out, &m)?)
    };
    let mut raw_bytes = 0u64;
    let mut encoded_bytes = 0u64;
    for (p, g) in coded.into_iter().flatten() {
        for w in [p, g] {
            let s = w.finish()?;
            raw_bytes += s["raw_bytes"].as_u64().unwrap();
            encoded_bytes += s["encoded_bytes"].as_u64().unwrap();
        }
    }
    if let Some(writers) = raw_writers {
        for (p, g) in writers {
            for writer in [p, g] {
                let report = writer.finish()?;
                if raw_only {
                    raw_bytes += report["raw_bytes"].as_u64().context("Raw size")?;
                    encoded_bytes += report["encoded_bytes"].as_u64().context("Encoded size")?;
                }
            }
        }
        let mut raw_metadata = m.clone();
        raw_metadata.as_object_mut().unwrap().remove("storage");
        raw_metadata["staging"] = json!({"encoding":"float32_bits"});
        atomic(raw.unwrap_or(out).join("metadata.json"), &raw_metadata)?;
    }
    m.as_object_mut().unwrap().remove("storage");
    m["staging"] = if raw_only {
        json!({"encoding":"float32_bits"})
    } else {
        json!({"encoding":ENCODING,"dictionary":info})
    };
    atomic(out.join("metadata.json"), &m)?;
    let report = json!({"source_database":database,"source_table":table,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"cells":nc,"logical_rows":nc*nt,"raw_bytes":raw_bytes,"stage_bytes":encoded_bytes,"coded_stage_bytes":if raw_only {None}else{Some(encoded_bytes)},"raw_only":raw_only,"dictionary":info,"read_seconds":read_seconds,"dictionary_encode_seconds":encode_seconds,"stage_seconds":stage_seconds,"seconds":started.elapsed().as_secs_f64(),"source_preparation_not_loader_timing":true});
    atomic(out.join("preparation.json"), &report)?;
    Ok(report)
}

#[cfg(test)]
mod mapwise_tests {
    use super::*;
    #[test]
    fn mapwise_codes_preserve_every_bit_axis_and_partial_frame() -> Result<()> {
        let temp = tempfile::tempdir()?;
        let raw = temp.path().join("raw");
        fs::create_dir(&raw)?;
        let nc = 4099;
        let ids: Vec<_> = (0..nc as u32).map(|c| c * 3 + 11).collect();
        stage::save_cells(&raw, &ids, &vec![41.; nc], &vec![-74.; nc])?;
        let m = json!({"model":"gfs","cells":nc,"leads":[0,3,9],"levels":[1000,500],"variables":["temperature","wind_u"],"surface_variables":["temperature_2m","dew_point_temperature_2m"],"staging":{"encoding":"float32_bits"}});
        atomic(raw.join("metadata.json"), &m)?;
        let bits = [
            0, 0x80000000, 1, 0x80000001, 0x7f7fffff, 0xff7fffff, 0x3f812345,
        ];
        for (t, lead) in [0, 3, 9].iter().enumerate() {
            for (prefix, shape, width) in [("lead", vec![nc, 2, 2], 4), ("surface", vec![nc, 2], 2)]
            {
                let values: Vec<_> = (0..nc * width)
                    .map(|i| f32::from_bits(bits[(i + t) % bits.len()]))
                    .collect();
                stage::write(
                    &raw.join(format!("{prefix}-{lead:03}.npy")),
                    &shape,
                    &values,
                )?;
            }
        }
        let coded = temp.path().join("coded");
        let report = encode_stage(&raw, &coded)?;
        assert_eq!(report["lookup_lifetime"], "one_spatial_map");
        let meta = read_json(coded.join("metadata.json"))?;
        assert_eq!(meta["staging"]["complete"], true);
        let dictionary = Dictionary::load(&coded, &meta)?.context("Missing dictionary")?;
        for field in &dictionary.fields {
            for palette in field {
                assert!(palette.windows(2).all(|v| f32::from_bits(v[0])
                    .total_cmp(&f32::from_bits(v[1]))
                    .is_lt()));
            }
        }
        for (t, lead) in [0, 3, 9].iter().enumerate() {
            for (prefix, shape, width) in [("lead", vec![nc, 2, 2], 4), ("surface", vec![nc, 2], 2)]
            {
                let file = stage::Array::open_as(
                    &coded.join(format!("{prefix}-{lead:03}.npy")),
                    &shape,
                    "uint32_le",
                )?;
                assert!(file.short_codes());
                let mut codes = vec![0f32; nc * width];
                file.read_into(0, &mut codes)?;
                for (i, code) in codes.iter().enumerate() {
                    let slot = i % width;
                    let (f, position) = if prefix == "lead" {
                        (slot % 2, (slot / 2) * 3 + t)
                    } else {
                        (2 + slot, t)
                    };
                    assert_eq!(
                        dictionary.value(f, position, code.to_bits())?,
                        bits[(i + t) % bits.len()]
                    );
                }
            }
        }
        assert_eq!(
            stage::load_cells(&raw)?["cell_id"].bytes,
            stage::load_cells(&coded)?["cell_id"].bytes
        );
        Ok(())
    }
    #[test]
    fn mapwise_rejects_nonfinite_wide_palettes_and_oversized_matrices() -> Result<()> {
        let temp = tempfile::tempdir()?;
        for (name, nc) in [("nonfinite", 3), ("wide", 65537)] {
            let raw = temp.path().join(name);
            fs::create_dir(&raw)?;
            let ids: Vec<_> = (0..nc as u32).collect();
            stage::save_cells(&raw, &ids, &vec![41.; nc], &vec![-74.; nc])?;
            let m = json!({"cells":nc,"leads":[0],"levels":[1000],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
            atomic(raw.join("metadata.json"), &m)?;
            let mut values: Vec<_> = (1..=nc as u32).map(f32::from_bits).collect();
            if name == "nonfinite" {
                values[1] = f32::NAN;
            }
            stage::write(&raw.join("lead-000.npy"), &[nc, 1, 1], &values)?;
            let out = temp.path().join(format!("{name}_out"));
            assert!(encode_stage(&raw, &out).is_err());
            assert_eq!(
                read_json(out.join("metadata.json"))?["staging"]["complete"],
                false
            );
            assert!(!out.join("values.dict.zst").exists());
        }
        let raw = temp.path().join("oversized");
        fs::create_dir(&raw)?;
        let m = json!({"cells":2_000_000,"leads":[0],"levels":vec![1000;1024],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
        atomic(raw.join("metadata.json"), &m)?;
        let error = encode_stage(&raw, &temp.path().join("oversized_out")).unwrap_err();
        assert!(error.to_string().contains("matrix exceeds"));
        Ok(())
    }
}
