//! One-time conversion of resident cloud files, preserving every weather book.
use crate::{cloud_coverage, clouds, dictionary::file_hash, runs, stage, state::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::HashMap, fs, path::Path};

pub const LEGACY_FILES: [&str; 2] = ["native-clouds.stage.zst", "native-clouds.stage.json"];

pub fn legacy_array(path: &Path, meta: &Value) -> Result<stage::Array> {
    let c = &meta["native_clouds"];
    let cells = meta["cells"].as_u64().context("Cloud cells")? as usize;
    anyhow::ensure!(
        meta["model"] == "hrrr"
            && cells > 0
            && cells <= 2_000_000
            && c["version"] == 1
            && c["shape"] == json!([cells, 50, 3])
            && c["fields"] == json!(clouds::FIELDS)
            && c["units"] == clouds::units()
            && c["model_levels"] == json!((1..=50).collect::<Vec<_>>())
            && c["full_values_checked"] == true
            && c["pressure_snapshot_id"] == meta["snapshot_id"]
            && c["snapshot_id"].as_str().is_some_and(|s| !s.is_empty())
            && c["leads"].as_array().map(Vec::len) == Some(1),
        "Invalid original cloud schema"
    );
    for key in ["model", "dataset", "init_time", "leads", "grid"] {
        anyhow::ensure!(
            c[key] == meta[key],
            "Original cloud identity changed: {key}"
        );
    }
    anyhow::ensure!(
        file_hash(&path.join(LEGACY_FILES[1]))?
            == c["index_sha256"]
                .as_str()
                .context("Original cloud index hash")?
            && fs::metadata(path.join(LEGACY_FILES[0]))?.len()
                == c["data_bytes"].as_u64().context("Original cloud bytes")?,
        "Original cloud file identity changed"
    );
    stage::Array::open(&path.join("native-clouds"), &[cells, 50, 3])
}

/// Compare all retained fraction/anchor bits, including every short final frame.
/// Publication invokes this again; a persisted success marker is insufficient.
pub fn verify(source: &Path, target: &Path, old: &Value, new: &Value) -> Result<Value> {
    anyhow::ensure!(
        old["native_clouds"]["version"] == 1
            && new["native_clouds"]["version"] == 2
            && old["native_clouds"]["snapshot_id"] == new["native_clouds"]["snapshot_id"]
            && new["native_clouds"]["conversion"]["source_metadata_sha256"]
                == file_hash(&source.join("metadata.json"))?,
        "Cloud conversion identity changed"
    );
    clouds::binding(target, new)?.context("Missing compacted cloud files")?;
    let cells = old["cells"].as_u64().context("Cloud cells")? as usize;
    let original = legacy_array(source, old)?;
    let coverage = cloud_coverage::Reader::open(target, cells)?;
    let anchors = stage::Array::open(&target.join("cloud-anchors"), &[cells, 2])?;
    let mut original_scratch = stage::Scratch::new()?;
    let mut fraction_scratch = cloud_coverage::Scratch::new()?;
    let mut anchor_scratch = stage::Scratch::new()?;
    let mut before = vec![0.; 4096 * 150];
    let mut after = vec![0.; 4096 * 50];
    let mut anchor = vec![0.; 4096 * 2];
    for first in (0..cells).step_by(4096) {
        let n = 4096.min(cells - first);
        original.read_into_with(first * 150, &mut before[..n * 150], &mut original_scratch)?;
        coverage.read_into(first, &mut after[..n * 50], &mut fraction_scratch)?;
        anchors.read_into_with(first * 2, &mut anchor[..n * 2], &mut anchor_scratch)?;
        for c in 0..n {
            anyhow::ensure!(
                anchor[c * 2].to_bits() == before[c * 150 + 2].to_bits()
                    && anchor[c * 2 + 1].to_bits() == before[c * 150 + 148].to_bits(),
                "Cloud conversion changed an anchor"
            );
            for l in 0..50 {
                anyhow::ensure!(
                    before[c * 150 + l * 3].to_bits() == after[c * 50 + l].to_bits(),
                    "Cloud conversion changed fraction at cell {}, level {}",
                    first + c,
                    l + 1
                );
            }
        }
    }
    Ok(
        json!({"all_fraction_bits_identical":true,"all_anchor_bits_identical":true,"fraction_values":cells*50,"anchor_values":cells*2}),
    )
}

fn convert(source: &Path, target: &Path, old: &Value) -> Result<Value> {
    let began = std::time::Instant::now();
    let cells = old["cells"].as_u64().context("Cloud cells")? as usize;
    let original = legacy_array(source, old)?;
    let mut fractions = vec![vec![0.; cells]; 50];
    let mut anchors = [vec![0.; cells], vec![0.; cells]];
    let mut buffer = vec![0.; 4096 * 150];
    let mut scratch = stage::Scratch::new()?;
    for first in (0..cells).step_by(4096) {
        let n = 4096.min(cells - first);
        original.read_into_with(first * 150, &mut buffer[..n * 150], &mut scratch)?;
        for c in 0..n {
            for l in 0..50 {
                fractions[l][first + c] = buffer[c * 150 + l * 3];
            }
            anchors[0][first + c] = buffer[c * 150 + 2];
            anchors[1][first + c] = buffer[c * 150 + 148];
            let top = buffer[c * 150 + 149];
            anyhow::ensure!(
                top.is_finite()
                    && (f64::from(top) - crate::cloud_geometry::top_pressure()).abs() < 0.1,
                "Original HRRR model top differs"
            );
        }
    }
    let encoded = fractions
        .iter()
        .map(|v| cloud_coverage::encode(v.iter().copied()))
        .collect::<Result<Vec<_>>>()?;
    drop(fractions);
    let mut coverage = cloud_coverage::write(target, &encoded)?;
    coverage["index_sha256"] = json!(file_hash(&target.join(cloud_coverage::INDEX))?);
    coverage["data_sha256"] = json!(file_hash(&target.join(cloud_coverage::DATA))?);
    let anchor_info = clouds::write_anchors(target, &anchors)?;
    let mut info = clouds::metadata(
        old,
        old["native_clouds"]["snapshot_id"]
            .as_str()
            .context("Cloud snapshot")?,
        coverage,
        anchor_info,
    )?;
    info["source_top_checked"] = json!(true);
    info["conversion"] = json!({"source_metadata_sha256":file_hash(&source.join("metadata.json"))?,"source_cloud_index_sha256":old["native_clouds"]["index_sha256"]});
    let mut next = old.clone();
    next["native_clouds"] = info;
    let proof = verify(source, target, old, &next)?;
    next["native_clouds"]["conversion"]["proof"] = proof;
    next["native_clouds"]["conversion"]["seconds"] = json!(began.elapsed().as_secs_f64());
    atomic(target.join("metadata.json"), &next)?;
    Ok(next)
}

pub fn compact(root: &Path) -> Result<Value> {
    let _update = Lock::acquire(root, "update", true, false)?;
    let original = catalog(root)?;
    let mut converted: HashMap<String, runs::Book> = HashMap::new();
    let mut replacements = Vec::new();
    let mut before_bytes = 0u64;
    let mut after_bytes = 0u64;
    for prior in runs::direct(&original, false) {
        let old_path = descriptor(root, &prior)?;
        let mut manifest = read_json(old_path.join("metadata.json"))?;
        if manifest["model"] != "hrrr" {
            continue;
        }
        let mut books = runs::books(&manifest)?;
        let mut changed = false;
        for book in &mut books {
            if let Some(new) = converted.get(&book.revision) {
                *book = new.clone();
                changed = true;
                continue;
            }
            let (old_entry, source, old) = runs::book_entry(root, book)?;
            if old["native_clouds"]["version"] != 1 {
                continue;
            }
            let (target, mut child) = candidate(root, "hrrr")?;
            child.table = old_entry.table;
            child.database = old_entry.database;
            child.owned = false;
            atomic(target.join("entry.json"), &child)?;
            stage::copy_cells(&source, &target)?;
            for name in [
                "validated.json",
                "source-validation.json",
                "values.dict.zst",
            ] {
                fs::hard_link(source.join(name), target.join(name))?;
            }
            let next = convert(&source, &target, &old)?;
            let mut new = book.clone();
            new.revision = child.revision;
            new.metadata_sha256 = file_hash(&target.join("metadata.json"))?;
            before_bytes += fs::metadata(source.join(LEGACY_FILES[0]))?.len()
                + fs::metadata(source.join(LEGACY_FILES[1]))?.len();
            after_bytes += clouds::FILES
                .iter()
                .map(|name| fs::metadata(target.join(name)).map(|m| m.len()))
                .collect::<std::io::Result<Vec<_>>>()?
                .iter()
                .sum::<u64>();
            converted.insert(book.revision.clone(), new.clone());
            *book = new;
            changed = true;
            println!(
                "{}",
                json!({"phase":"cloud_hour_compacted","lead":book.lead,"init_time":next["init_time"],"bytes":next["native_clouds"]["data_bytes"]})
            );
        }
        if !changed {
            continue;
        }
        let (path, mut next) = candidate(root, "hrrr")?;
        next.table = prior.table.clone();
        next.database = prior.database.clone();
        next.owned = prior.owned;
        atomic(path.join("entry.json"), &next)?;
        manifest["storage"]["books"] = json!(books);
        atomic(path.join("metadata.json"), &manifest)?;
        stage::copy_cells(&old_path, &path)?;
        let proof = runs::validate_manifest(root, &next, &manifest, false)?;
        atomic(path.join("validated.json"), &proof)?;
        replacements.push((prior, next));
    }
    let incoming = root.join("incoming/revisions");
    let mut incoming_converted = 0;
    if incoming.exists() {
        for entry in fs::read_dir(incoming)? {
            let path = entry?.path();
            let meta = path.join("metadata.json");
            if !meta.exists() {
                continue;
            }
            let old = read_json(&meta)?;
            if old["model"] == "hrrr" && old["native_clouds"]["version"] == 1 {
                convert(&path, &path, &old)?;
                for name in LEGACY_FILES {
                    fs::remove_file(path.join(name))?;
                }
                incoming_converted += 1;
            }
        }
    }
    if !replacements.is_empty() {
        runs::publish_cloud_replacements(root, &replacements)?;
    }
    Ok(
        json!({"converted_hours":converted.len(),"converted_manifests":replacements.len(),"converted_incoming_hours":incoming_converted,"old_cloud_bytes":before_bytes,"new_cloud_bytes":after_bytes,"reclaimed_after_retirement_grace_bytes":before_bytes.saturating_sub(after_bytes)}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_preserves_every_fraction_and_rejects_a_forged_success_proof() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("source");
        let target = root.path().join("target");
        fs::create_dir_all(&source).unwrap();
        fs::create_dir_all(&target).unwrap();
        let cells = 263;
        let mut values = Vec::new();
        for c in 0..cells {
            let bottom = 100000. + c as f64;
            for l in 0..50 {
                let fraction = if c == 0 && l == 0 {
                    -0.
                } else {
                    ((c + l) % 101) as f32 / 100.
                };
                values.extend_from_slice(&[
                    fraction,
                    50. + 550. * l as f32 + c as f32,
                    (bottom + (crate::cloud_geometry::top_pressure() - bottom) * l as f64 / 49.)
                        as f32,
                ]);
            }
        }
        stage::write(&source.join("native-clouds"), &[cells, 50, 3], &values).unwrap();
        let mut old = json!({"model":"hrrr","dataset":"noaa-hrrr-forecast-48-hour-virtual","init_time":"2026-10-19T00:00:00","snapshot_id":"pressure-snapshot","leads":[20],"cells":cells,"grid":{"test":true}});
        let mut info = json!({"version":1,"shape":[cells,50,3],"model_levels":(1..=50).collect::<Vec<_>>(),"fields":clouds::FIELDS,"units":clouds::units(),"full_values_checked":true,"snapshot_id":"cloud-snapshot","pressure_snapshot_id":old["snapshot_id"],"index_sha256":file_hash(&source.join(LEGACY_FILES[1])).unwrap(),"data_bytes":fs::metadata(source.join(LEGACY_FILES[0])).unwrap().len()});
        for k in ["model", "dataset", "init_time", "leads", "grid"] {
            info[k] = old[k].clone();
        }
        old["native_clouds"] = info;
        atomic(source.join("metadata.json"), &old).unwrap();
        let mut next = convert(&source, &target, &old).unwrap();
        let checked = verify(&source, &target, &old, &next).unwrap();
        assert_eq!(checked["fraction_values"], cells * 50);
        assert_eq!(read_json(source.join("metadata.json")).unwrap(), old);
        let index = target.join(cloud_coverage::INDEX);
        let mut altered = read_json(&index).unwrap();
        altered["palettes"][0][0] = json!(0.25f32.to_bits());
        atomic(&index, &altered).unwrap();
        next["native_clouds"]["coverage"]["index_sha256"] = json!(file_hash(&index).unwrap());
        // Shape, hashes, CRCs and producer success markers still pass; the
        // independent comparison must reject the changed source value.
        assert!(clouds::binding(&target, &next).is_ok());
        assert!(verify(&source, &target, &old, &next).is_err());
    }
}
