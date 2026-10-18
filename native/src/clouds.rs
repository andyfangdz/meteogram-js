//! Immutable HRRR model-level cloud supplement, independent of pressure profiles.
use crate::{dictionary::file_hash, stage, state::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub const LEVELS: usize = 50;
pub const STRIDE: usize = LEVELS * 3;
pub const FILES: [&str; 2] = ["native-clouds.stage.zst", "native-clouds.stage.json"];
pub const FIELDS: [&str; 3] = ["fraction_of_cloud_cover", "geopotential_height", "pressure"];
pub fn units() -> Value {
    json!({"fraction_of_cloud_cover":"1","geopotential_height":"m","pressure":"Pa"})
}
pub fn validate(values: &[f32]) -> Result<()> {
    anyhow::ensure!(
        values.len().is_multiple_of(STRIDE) && !values.is_empty(),
        "Invalid native cloud width"
    );
    for cell in values.as_chunks::<STRIDE>().0 {
        for v in cell.as_chunks::<3>().0 {
            anyhow::ensure!(
                v.iter().all(|x| x.is_finite()) && (0. ..=1.).contains(&v[0]) && v[2] > 0.,
                "Invalid native cloud value"
            );
        }
        let ascending = cell[4] > cell[1];
        for levels in cell.windows(6).step_by(3) {
            anyhow::ensure!(
                if ascending {
                    levels[4] > levels[1] && levels[5] < levels[2]
                } else {
                    levels[4] < levels[1] && levels[5] > levels[2]
                },
                "Native cloud heights/pressures are not monotonic"
            );
        }
    }
    Ok(())
}
pub struct Reader {
    array: stage::Array,
    pub snapshot: String,
    pressure_snapshot: String,
}
pub fn binding(path: &Path, meta: &Value) -> Result<Option<Reader>> {
    let Some(c) = meta.get("native_clouds") else {
        return Ok(None);
    };
    let cells = meta["cells"].as_u64().context("Cloud cells")? as usize;
    anyhow::ensure!(
        meta["model"] == "hrrr"
            && cells > 0
            && cells <= 2_000_000
            && c["version"] == 1
            && c["shape"] == json!([cells, LEVELS, 3])
            && c["fields"] == json!(FIELDS)
            && c["units"] == units()
            && c["model_levels"] == json!((1..=LEVELS).collect::<Vec<_>>())
            && c["full_values_checked"] == true,
        "Invalid native cloud schema/proof"
    );
    for key in ["model", "dataset", "init_time", "leads", "grid"] {
        anyhow::ensure!(
            c[key] == meta[key],
            "Native cloud source identity differs: {key}"
        );
    }
    anyhow::ensure!(
        c["pressure_snapshot_id"] == meta["snapshot_id"]
            && c["snapshot_id"].as_str().is_some_and(|s| !s.is_empty()),
        "Cloud/pressure provenance mismatch"
    );
    anyhow::ensure!(
        c["leads"].as_array().map(Vec::len) == Some(1),
        "Cloud supplement requires one hour"
    );
    anyhow::ensure!(
        file_hash(&path.join(FILES[1]))?
            == c["index_sha256"].as_str().context("Cloud index hash")?
            && fs::metadata(path.join(FILES[0]))?.len()
                == c["data_bytes"].as_u64().context("Cloud bytes")?,
        "Cloud index/file identity differs"
    );
    Ok(Some(Reader {
        pressure_snapshot: meta["snapshot_id"]
            .as_str()
            .context("Pressure snapshot")?
            .to_owned(),
        array: stage::Array::open(&path.join("native-clouds.npy"), &[cells, LEVELS, 3])?,
        snapshot: c["snapshot_id"]
            .as_str()
            .context("Cloud snapshot")?
            .to_owned(),
    }))
}
impl Reader {
    pub fn profile(&self, cell: usize, scratch: &mut stage::Scratch) -> Result<[Vec<f32>; 3]> {
        let mut values = [0.; STRIDE];
        self.array.read_into_with(
            cell.checked_mul(STRIDE).context("Cloud offset")?,
            &mut values,
            scratch,
        )?;
        validate(&values)?;
        Ok(std::array::from_fn(|f| {
            values.as_chunks::<3>().0.iter().map(|v| v[f]).collect()
        }))
    }
}
pub fn copy(source: &Path, target: &Path, meta: &Value) -> Result<()> {
    if binding(source, meta)?.is_none() {
        return Ok(());
    }
    for name in FILES {
        fs::hard_link(source.join(name), target.join(name))?;
        fs::File::open(target.join(name))?.sync_all()?;
    }
    Ok(())
}
pub fn readers(root: &Path, meta: &Value) -> Result<BTreeMap<u16, Reader>> {
    let mut result = BTreeMap::new();
    if partitioned(meta) {
        for book in crate::runs::books(meta)? {
            let (_, path, child) = crate::runs::book_entry(root, &book)?;
            if let Some(reader) = binding(&path, &child)? {
                result.insert(book.lead, reader);
            }
        }
    }
    Ok(result)
}
pub fn payload(readers: &BTreeMap<u16, Reader>, cell: usize) -> Result<Option<Value>> {
    let mut scratch = stage::Scratch::new()?;
    let mut rows = Vec::new();
    let mut sources = Vec::new();
    let mut leads = Vec::new();
    for (lead, reader) in readers {
        let [cloud, height, pressure] = reader.profile(cell, &mut scratch)?;
        rows.push(json!([lead, cloud, height, pressure]));
        leads.push(*lead);
        sources.push(json!({"lead":lead,"snapshot_id":reader.snapshot,"pressure_snapshot_id":reader.pressure_snapshot}));
    }
    Ok((!rows.is_empty()).then(||json!({"model_levels":(1..=LEVELS).collect::<Vec<_>>(),"fields":["lead_hour",FIELDS[0],FIELDS[1],FIELDS[2]],"units":units(),"available_leads":leads,"lead_sources":sources,"rows":rows})))
}

/// A cloud-only revision must preserve every pressure value, proof and hour.
pub fn check_upgrade(root: &Path, prior: &Entry, next: &Entry) -> Result<()> {
    anyhow::ensure!(
        prior.database == next.database && prior.table == next.table && prior.owned == next.owned,
        "Cloud upgrade changed pressure table ownership"
    );
    let old = read_json(descriptor(root, prior)?.join("metadata.json"))?;
    let new = read_json(descriptor(root, next)?.join("metadata.json"))?;
    let mut a = old.clone();
    let mut b = new.clone();
    a["storage"]
        .as_object_mut()
        .context("Old storage")?
        .remove("books");
    b["storage"]
        .as_object_mut()
        .context("New storage")?
        .remove("books");
    anyhow::ensure!(
        a == b && old["model"] == "hrrr",
        "Cloud upgrade changed pressure manifest"
    );
    let old_books = crate::runs::books(&old)?;
    let new_books = crate::runs::books(&new)?;
    anyhow::ensure!(
        old_books.len() == new_books.len(),
        "Cloud upgrade changed forecast hours"
    );
    let mut changed = 0;
    for (a, b) in old_books.iter().zip(&new_books) {
        if a == b {
            continue;
        }
        anyhow::ensure!(
            a.lead == b.lead && a.entries == b.entries && a.snapshot_id == b.snapshot_id,
            "Cloud upgrade changed hour identity"
        );
        let (ae, ap, mut am) = crate::runs::book_entry(root, a)?;
        let (be, bp, mut bm) = crate::runs::book_entry(root, b)?;
        anyhow::ensure!(
            ae.database == be.database
                && ae.table == be.table
                && am.get("native_clouds").is_none()
                && binding(&bp, &bm)?.is_some(),
            "Cloud upgrade replaced existing clouds or pressure table"
        );
        am.as_object_mut().unwrap().remove("native_clouds");
        bm.as_object_mut().unwrap().remove("native_clouds");
        anyhow::ensure!(am == bm, "Cloud upgrade changed pressure metadata");
        for name in [
            "validated.json",
            "source-validation.json",
            "values.dict.zst",
        ] {
            anyhow::ensure!(
                file_hash(&ap.join(name))? == file_hash(&bp.join(name))?,
                "Cloud upgrade changed pressure proof/codebook"
            );
        }
        let coordinates = ["cells.bin.zst", "cells.npz"]
            .into_iter()
            .find(|name| ap.join(name).exists())
            .context("Missing pressure coordinates")?;
        anyhow::ensure!(
            file_hash(&ap.join(coordinates))? == file_hash(&bp.join(coordinates))?,
            "Cloud upgrade changed pressure coordinates"
        );
        changed += 1;
    }
    anyhow::ensure!(changed > 0, "Cloud upgrade did not add clouds");
    Ok(())
}
pub async fn backfill(root: &Path, run: &str, leads: Option<Vec<u16>>) -> Result<Value> {
    let _lock = Lock::acquire(root, "update", true, false)?;
    let c = catalog(root)?;
    let mut parent = match run {
        "latest" => c.current.get("hrrr"),
        "latest_complete" => c.complete_runs.get("hrrr").and_then(|v| v.first()),
        "long" => c.hrrr_long.as_ref(),
        _ => anyhow::bail!("Choose latest, latest_complete or long"),
    }
    .context("HRRR run unavailable")?
    .clone();
    let initial = read_json(descriptor(root, &parent)?.join("metadata.json"))?;
    anyhow::ensure!(
        partitioned(&initial),
        "Backfill requires an incremental HRRR run"
    );
    let mut added = Vec::new();
    for book in crate::runs::books(&initial)? {
        if leads.as_ref().is_some_and(|v| !v.contains(&book.lead)) {
            continue;
        }
        let (_, old_path, old_meta) = crate::runs::book_entry(root, &book)?;
        if binding(&old_path, &old_meta)?.is_some() {
            continue;
        }
        let (path, mut child) = candidate(root, "hrrr")?;
        child.table = parent.table.clone();
        child.owned = false;
        atomic(path.join("entry.json"), &child)?;
        atomic(path.join("metadata.json"), &old_meta)?;
        crate::stage::copy_cells(&old_path, &path)?;
        for name in [
            "validated.json",
            "source-validation.json",
            "values.dict.zst",
        ] {
            fs::hard_link(old_path.join(name), path.join(name))?;
        }
        // This candidate is private and recoverable. Publication is last.
        let info = crate::source::fetch_clouds(&path, None).await?;
        let (parent_path, mut next) = candidate(root, "hrrr")?;
        next.table = parent.table.clone();
        atomic(parent_path.join("entry.json"), &next)?;
        let mut manifest = read_json(descriptor(root, &parent)?.join("metadata.json"))?;
        let mut books = crate::runs::books(&manifest)?;
        let target = books
            .iter_mut()
            .find(|b| b.lead == book.lead)
            .context("Backfill hour vanished")?;
        target.revision = child.revision;
        target.metadata_sha256 = file_hash(&path.join("metadata.json"))?;
        manifest["storage"]["books"] = json!(books);
        atomic(parent_path.join("metadata.json"), &manifest)?;
        crate::stage::copy_cells(&old_path, &parent_path)?;
        fs::File::open(&path)?.sync_all()?;
        let proof = crate::runs::validate_manifest(root, &next, &manifest, false)?;
        atomic(parent_path.join("validated.json"), &proof)?;
        crate::runs::publish_cloud_upgrade(root, &parent, next.clone())?;
        parent = next;
        added.push(book.lead);
        println!(
            "{}",
            json!({"phase":"native_clouds_published","lead":book.lead,"revision":parent.revision,"data_bytes":info["data_bytes"]})
        );
    }
    Ok(json!({"revision":parent.revision,"native_cloud_leads_added":added}))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(path: &Path, cells: usize) -> (Value, Vec<f32>) {
        fs::create_dir_all(path).unwrap();
        let values = (0..cells)
            .flat_map(|c| {
                (0..LEVELS).flat_map(move |l| {
                    [
                        if l == 0 { 0. } else { 0.8 },
                        100. + l as f32 * 200. + c as f32,
                        100000. - l as f32 * 1500.,
                    ]
                })
            })
            .collect::<Vec<_>>();
        let shape = [cells, LEVELS, 3];
        stage::write(&path.join("native-clouds.npy"), &shape, &values).unwrap();
        let mut meta = json!({"model":"hrrr","dataset":"noaa-hrrr-forecast-18-hour-virtual","init_time":"2026-10-18T21:00:00","snapshot_id":"test_snapshot","leads":[0],"cells":cells,"grid":{"test":true}});
        let mut cloud = json!({"version":1,"shape":shape,"model_levels":(1..=LEVELS).collect::<Vec<_>>(),"fields":FIELDS,"units":units(),"full_values_checked":true,"data_bytes":fs::metadata(path.join(FILES[0])).unwrap().len(),"index_sha256":file_hash(&path.join(FILES[1])).unwrap()});
        for key in [
            "model",
            "dataset",
            "init_time",
            "snapshot_id",
            "leads",
            "grid",
        ] {
            cloud[key] = meta[key].clone();
        }
        cloud["pressure_snapshot_id"] = meta["snapshot_id"].clone();
        meta["native_clouds"] = cloud;
        (meta, values)
    }
    #[test]
    fn native_cloud_roundtrip_partial_frame_and_random_cell() {
        let root = tempfile::tempdir().unwrap();
        let (meta, values) = fixture(root.path(), 4100);
        validate(&values).unwrap();
        let reader = binding(root.path(), &meta).unwrap().unwrap();
        let mut scratch = stage::Scratch::new().unwrap();
        for cell in [4099, 0, 4095, 4096] {
            let [cloud, height, pressure] = reader.profile(cell, &mut scratch).unwrap();
            for l in 0..LEVELS {
                for (f, v) in [cloud[l], height[l], pressure[l]].iter().enumerate() {
                    assert_eq!(v.to_bits(), values[cell * STRIDE + l * 3 + f].to_bits());
                }
            }
        }
        assert!(reader.profile(4100, &mut scratch).is_err());
    }
    #[test]
    fn native_cloud_rejects_missing_fraction_bad_geometry_and_source_binding() {
        let root = tempfile::tempdir().unwrap();
        let (meta, values) = fixture(root.path(), 1);
        for (index, value) in [(0, f32::NAN), (0, 1.01), (2, -1.), (4, 100.), (5, 100000.)] {
            let mut broken = values.clone();
            broken[index] = value;
            assert!(validate(&broken).is_err());
        }
        for key in ["pressure_snapshot_id", "grid", "leads", "units", "shape"] {
            let mut broken = meta.clone();
            broken["native_clouds"][key] = json!("wrong");
            assert!(binding(root.path(), &broken).is_err());
        }
        fs::write(root.path().join(FILES[1]), b"{}").unwrap();
        assert!(binding(root.path(), &meta).is_err());
    }
    #[test]
    fn native_cloud_crc_is_checked_for_every_point_read() {
        let root = tempfile::tempdir().unwrap();
        let (meta, _) = fixture(root.path(), 1);
        let reader = binding(root.path(), &meta).unwrap().unwrap();
        let path = root.path().join(FILES[0]);
        let mut data = fs::read(&path).unwrap();
        let n = data.len();
        data[n - 1] ^= 1;
        fs::write(path, data).unwrap();
        assert!(reader
            .profile(0, &mut stage::Scratch::new().unwrap())
            .is_err());
    }
    #[test]
    fn cloud_files_follow_catalog_retention_and_hardlink_ownership() {
        let root = tempfile::tempdir().unwrap();
        let (path, e) = candidate(root.path(), "hrrr").unwrap();
        let (meta, _) = fixture(&path, 1);
        atomic(path.join("metadata.json"), &meta).unwrap();
        let shared = root.path().join("shared");
        fs::create_dir(&shared).unwrap();
        copy(&path, &shared, &meta).unwrap();
        let mut c = Catalog::default();
        c.current.insert(
            "hrrr".into(),
            Entry {
                owned: false,
                ..e.clone()
            },
        );
        // The candidate descriptor itself must use non-owned storage for this disk-only test.
        let e = Entry { owned: false, ..e };
        atomic(path.join("entry.json"), &e).unwrap();
        atomic(root.path().join("catalog.json"), &c).unwrap();
        crate::store::discard(root.path(), &path).unwrap();
        assert!(path.join(FILES[0]).exists());
        atomic(root.path().join("catalog.json"), &Catalog::default()).unwrap();
        crate::store::discard(root.path(), &path).unwrap();
        assert!(!path.join(FILES[0]).exists());
        binding(&shared, &meta)
            .unwrap()
            .unwrap()
            .profile(0, &mut stage::Scratch::new().unwrap())
            .unwrap();
    }
    #[test]
    fn cloud_upgrade_rejects_pressure_changes_and_replacement() {
        let root = tempfile::tempdir().unwrap();
        let (old_path, mut old_child) = candidate(root.path(), "hrrr").unwrap();
        old_child.owned = false;
        atomic(old_path.join("entry.json"), &old_child).unwrap();
        let (new_path, mut new_child) = candidate(root.path(), "hrrr").unwrap();
        new_child.owned = false;
        new_child.table = old_child.table.clone();
        atomic(new_path.join("entry.json"), &new_child).unwrap();
        let (new_meta, _) = fixture(&new_path, 1);
        let mut old_meta = new_meta.clone();
        old_meta.as_object_mut().unwrap().remove("native_clouds");
        atomic(old_path.join("metadata.json"), &old_meta).unwrap();
        atomic(new_path.join("metadata.json"), &new_meta).unwrap();
        for name in [
            "validated.json",
            "source-validation.json",
            "values.dict.zst",
            "cells.bin.zst",
        ] {
            fs::write(old_path.join(name), b"same-pressure-proof").unwrap();
            fs::hard_link(old_path.join(name), new_path.join(name)).unwrap();
        }
        let book = |path: &Path, e: &Entry| crate::runs::Book {
            lead: 0,
            revision: e.revision.clone(),
            entries: 1,
            metadata_sha256: file_hash(&path.join("metadata.json")).unwrap(),
            snapshot_id: "test_snapshot".into(),
        };
        let (a, mut ae) = candidate(root.path(), "hrrr").unwrap();
        let (b, mut be) = candidate(root.path(), "hrrr").unwrap();
        ae.table = old_child.table.clone();
        be.table = ae.table.clone();
        let mut manifest = old_meta.clone();
        manifest["storage"] = json!({"books":[book(&old_path,&old_child)]});
        atomic(a.join("metadata.json"), &manifest).unwrap();
        manifest["storage"]["books"] = json!([book(&new_path, &new_child)]);
        atomic(b.join("metadata.json"), &manifest).unwrap();
        check_upgrade(root.path(), &ae, &be).unwrap();
        manifest["leads"] = json!([0, 1]);
        atomic(b.join("metadata.json"), &manifest).unwrap();
        assert!(check_upgrade(root.path(), &ae, &be).is_err());
        manifest["leads"] = json!([0]);
        atomic(b.join("metadata.json"), &manifest).unwrap();
        // A second replacement is forbidden even with identical native values.
        atomic(old_path.join("metadata.json"), &new_meta).unwrap();
        let mut old_manifest = manifest.clone();
        old_manifest["storage"]["books"] = json!([book(&old_path, &old_child)]);
        atomic(a.join("metadata.json"), &old_manifest).unwrap();
        assert!(check_upgrade(root.path(), &ae, &be).is_err());
    }
}
