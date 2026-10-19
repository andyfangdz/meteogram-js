//! Immutable HRRR cloud coverage and native anchors, with derived coordinates.
use crate::{dictionary::file_hash, stage, state::*};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{collections::BTreeMap, fs, path::Path};

pub const LEVELS: usize = 50;
pub const FILES: [&str; 4] = [
    crate::cloud_coverage::DATA,
    crate::cloud_coverage::INDEX,
    "cloud-anchors.stage.zst",
    "cloud-anchors.stage.json",
];
/// Conservative bound for both data/index pairs plus provenance. Fractions may
/// use UInt16 codes and full 65,536-entry Float32 palettes at every level.
/// Include pretty-printed JSON whitespace in the per-palette-entry bound.
pub fn max_stored_bytes(cells: usize) -> Result<u64> {
    anyhow::ensure!((1..=2_000_000).contains(&cells), "Cloud cell count");
    Ok((cells * (LEVELS * 2 + 8)
        + cells.div_ceil(256) * 1024
        + cells.div_ceil(stage::BLOCK_CELLS) * 1024
        + LEVELS * 65_536 * 24
        + 1024 * 1024) as u64)
}

pub fn write_anchors(path: &Path, anchors: &[Vec<f32>; 2]) -> Result<Value> {
    let started = std::time::Instant::now();
    let cells = anchors[0].len();
    anyhow::ensure!(
        cells > 0 && cells <= 2_000_000 && anchors[1].len() == cells,
        "Cloud anchor shape"
    );
    let mut values = Vec::with_capacity(cells * 2);
    for (&p, &h) in anchors[0].iter().zip(&anchors[1]) {
        anyhow::ensure!(
            p.is_finite() && p > 10000. && p < 120000. && h.is_finite() && h > 20000. && h < 40000.,
            "Invalid cloud anchor"
        );
        values.extend_from_slice(&[p, h]);
    }
    let mut info = stage::write(&path.join("cloud-anchors"), &[cells, 2], &values)?;
    let reader = stage::Array::open(&path.join("cloud-anchors"), &[cells, 2])?;
    let mut scratch = stage::Scratch::new()?;
    let mut buffer = vec![0.; 4096 * 2];
    for first in (0..cells).step_by(4096) {
        let n = 4096.min(cells - first);
        reader.read_into_with(first * 2, &mut buffer[..n * 2], &mut scratch)?;
        anyhow::ensure!(
            buffer[..n * 2]
                .iter()
                .zip(&values[first * 2..(first + n) * 2])
                .all(|(a, b)| a.to_bits() == b.to_bits()),
            "Cloud anchor roundtrip"
        );
    }
    info["data_bytes"] = json!(fs::metadata(path.join(FILES[2]))?.len());
    info["index_bytes"] = json!(fs::metadata(path.join(FILES[3]))?.len());
    info["index_sha256"] = json!(file_hash(&path.join(FILES[3]))?);
    info["data_sha256"] = json!(file_hash(&path.join(FILES[2]))?);
    info["full_values_checked"] = json!(true);
    info["seconds"] = json!(started.elapsed().as_secs_f64());
    Ok(info)
}

pub fn metadata(meta: &Value, snapshot: &str, coverage: Value, anchors: Value) -> Result<Value> {
    let cells = meta["cells"].as_u64().context("Cloud cells")?;
    anyhow::ensure!(
        cells > 0
            && cells <= 2_000_000
            && coverage["full_values_checked"] == true
            && anchors["full_values_checked"] == true,
        "Incomplete cloud storage proof"
    );
    let data_bytes = coverage["data_bytes"]
        .as_u64()
        .context("Coverage bytes")?
        .checked_add(anchors["data_bytes"].as_u64().context("Anchor bytes")?)
        .context("Cloud byte overflow")?;
    let mut result = json!({"version":2,"format":"cloud_fraction_anchors_v2","shape":[cells,50,3],"model_levels":(1..=50).collect::<Vec<_>>(),"fields":FIELDS,"units":units(),"full_values_checked":true,"stored_fraction_values":cells*50,"stored_anchor_values":cells*2,"geometry":crate::cloud_geometry::description(),"coverage":coverage,"anchors":anchors,"snapshot_id":snapshot,"pressure_snapshot_id":meta["snapshot_id"],"data_bytes":data_bytes});
    for key in ["model", "dataset", "init_time", "leads", "cells", "grid"] {
        result[key] = meta[key].clone();
    }
    Ok(result)
}
pub const FIELDS: [&str; 3] = ["fraction_of_cloud_cover", "geopotential_height", "pressure"];
pub fn units() -> Value {
    json!({"fraction_of_cloud_cover":"1","geopotential_height":"m","pressure":"Pa"})
}
pub struct Reader {
    coverage: crate::cloud_coverage::Reader,
    anchors: stage::Array,
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
            && c["version"] == 2
            && c["shape"] == json!([cells, LEVELS, 3])
            && c["fields"] == json!(FIELDS)
            && c["units"] == units()
            && c["model_levels"] == json!((1..=LEVELS).collect::<Vec<_>>())
            && c["full_values_checked"] == true
            && c["source_top_checked"] == true,
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
            && c["snapshot_id"].as_str().is_some_and(|s| !s.is_empty())
            && c["leads"].as_array().map(Vec::len) == Some(1),
        "Cloud/pressure provenance mismatch"
    );
    anyhow::ensure!(
        c["geometry"] == crate::cloud_geometry::description()
            && c["stored_fraction_values"] == json!(cells * LEVELS)
            && c["stored_anchor_values"] == json!(cells * 2)
            && file_hash(&path.join(FILES[1]))?
                == c["coverage"]["index_sha256"]
                    .as_str()
                    .context("Coverage index hash")?
            && file_hash(&path.join(FILES[3]))?
                == c["anchors"]["index_sha256"]
                    .as_str()
                    .context("Anchor index hash")?
            && fs::metadata(path.join(FILES[0]))?.len()
                == c["coverage"]["data_bytes"]
                    .as_u64()
                    .context("Coverage bytes")?
            && fs::metadata(path.join(FILES[2]))?.len()
                == c["anchors"]["data_bytes"]
                    .as_u64()
                    .context("Anchor bytes")?,
        "Cloud file/geometry identity differs"
    );
    Ok(Some(Reader {
        coverage: crate::cloud_coverage::Reader::open(path, cells)?,
        anchors: stage::Array::open(&path.join("cloud-anchors"), &[cells, 2])?,
        pressure_snapshot: meta["snapshot_id"]
            .as_str()
            .context("Pressure snapshot")?
            .to_owned(),
        snapshot: c["snapshot_id"]
            .as_str()
            .context("Cloud snapshot")?
            .to_owned(),
    }))
}
impl Reader {
    pub fn stored(
        &self,
        cell: usize,
        scratch: &mut stage::Scratch,
        coverage_scratch: &mut crate::cloud_coverage::Scratch,
    ) -> Result<(Vec<f32>, [f32; 2])> {
        let mut anchor = [0.; 2];
        self.anchors.read_into_with(
            cell.checked_mul(2).context("Anchor offset")?,
            &mut anchor,
            scratch,
        )?;
        Ok((self.coverage.profile(cell, coverage_scratch)?, anchor))
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
pub fn payload(
    readers: &BTreeMap<u16, Reader>,
    cell: usize,
    atmosphere: &Value,
) -> Result<Option<Value>> {
    let mut scratch = stage::Scratch::new()?;
    let mut coverage_scratch = crate::cloud_coverage::Scratch::new()?;
    let mut rows = Vec::new();
    let mut sources = Vec::new();
    let mut leads = Vec::new();
    for (lead, reader) in readers {
        let (cloud, anchors) = reader.stored(cell, &mut scratch, &mut coverage_scratch)?;
        let [height, pressure] = crate::cloud_geometry::from_payload(atmosphere, *lead, anchors)?;
        rows.push(json!([lead, cloud, height, pressure]));
        leads.push(*lead);
        sources.push(json!({"lead":lead,"snapshot_id":reader.snapshot,"pressure_snapshot_id":reader.pressure_snapshot}));
    }
    Ok((!rows.is_empty()).then(||json!({"model_levels":(1..=LEVELS).collect::<Vec<_>>(),"fields":["lead_hour",FIELDS[0],FIELDS[1],FIELDS[2]],"units":units(),"available_leads":leads,"lead_sources":sources,"geometry":crate::cloud_geometry::description(),"rows":rows})))
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
            ae.database == be.database && ae.table == be.table && binding(&bp, &bm)?.is_some(),
            "Cloud upgrade changed pressure table or lacks clouds"
        );
        if am.get("native_clouds").is_some() {
            crate::cloud_compaction::verify(&ap, &bp, &am, &bm)?;
        }
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
        let coordinates = ["cells.bin.zst"]
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
    const STRIDE: usize = LEVELS * 3;
    fn fixture(path: &Path, cells: usize) -> (Value, Vec<f32>) {
        fs::create_dir_all(path).unwrap();
        let values = (0..cells)
            .flat_map(|c| {
                (0..LEVELS).flat_map(move |l| {
                    [
                        if l == 0 { 0. } else { 0.8 },
                        100. + l as f32 * 600. + c as f32,
                        100000. - l as f32 * 1500.,
                    ]
                })
            })
            .collect::<Vec<_>>();
        let mut meta = json!({"model":"hrrr","dataset":"noaa-hrrr-forecast-18-hour-virtual","init_time":"2026-10-18T21:00:00","snapshot_id":"test_snapshot","leads":[0],"cells":cells,"grid":{"test":true}});
        let encoded = (0..LEVELS)
            .map(|level| {
                crate::cloud_coverage::encode((0..cells).map(|c| values[c * STRIDE + level * 3]))
            })
            .collect::<Result<Vec<_>>>()
            .unwrap();
        let mut coverage = crate::cloud_coverage::write(path, &encoded).unwrap();
        coverage["index_sha256"] = json!(file_hash(&path.join(FILES[1])).unwrap());
        coverage["data_sha256"] = json!(file_hash(&path.join(FILES[0])).unwrap());
        let anchors = [
            (0..cells).map(|c| values[c * STRIDE + 2]).collect(),
            (0..cells).map(|c| values[(c + 1) * STRIDE - 2]).collect(),
        ];
        let mut cloud = metadata(
            &meta,
            "test_snapshot",
            coverage,
            write_anchors(path, &anchors).unwrap(),
        )
        .unwrap();
        cloud["source_top_checked"] = json!(true);
        meta["native_clouds"] = cloud;
        (meta, values)
    }
    #[test]
    fn native_cloud_roundtrip_partial_frame_and_random_cell() {
        let root = tempfile::tempdir().unwrap();
        let (meta, values) = fixture(root.path(), 4100);
        let reader = binding(root.path(), &meta).unwrap().unwrap();
        let mut scratch = stage::Scratch::new().unwrap();
        for cell in [4099, 0, 4095, 4096] {
            let (cloud, anchors) = reader
                .stored(
                    cell,
                    &mut scratch,
                    &mut crate::cloud_coverage::Scratch::new().unwrap(),
                )
                .unwrap();
            for l in 0..LEVELS {
                assert_eq!(cloud[l].to_bits(), values[cell * STRIDE + l * 3].to_bits());
            }
            assert_eq!(anchors[0].to_bits(), values[cell * STRIDE + 2].to_bits());
            assert_eq!(
                anchors[1].to_bits(),
                values[(cell + 1) * STRIDE - 2].to_bits()
            );
        }
        assert!(reader
            .stored(
                4100,
                &mut scratch,
                &mut crate::cloud_coverage::Scratch::new().unwrap()
            )
            .is_err());
    }
    #[test]
    fn native_cloud_rejects_bad_anchors_and_source_binding() {
        let root = tempfile::tempdir().unwrap();
        let (meta, _) = fixture(root.path(), 1);
        for anchors in [
            [vec![f32::NAN], vec![30000.]],
            [vec![-1.], vec![30000.]],
            [vec![100000.], vec![1000.]],
        ] {
            assert!(write_anchors(root.path(), &anchors).is_err());
        }
        for key in [
            "pressure_snapshot_id",
            "grid",
            "leads",
            "units",
            "shape",
            "geometry",
            "source_top_checked",
            "version",
        ] {
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
            .stored(
                0,
                &mut stage::Scratch::new().unwrap(),
                &mut crate::cloud_coverage::Scratch::new().unwrap()
            )
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
        // The candidate descriptor itself must use non-owned storage for this disk-only test.
        let e = Entry { owned: false, ..e };
        atomic(path.join("entry.json"), &e).unwrap();
        let (parent_path, mut parent) = candidate(root.path(), "hrrr").unwrap();
        parent.owned = false;
        parent.table = e.table.clone();
        let mut manifest = meta.clone();
        manifest["storage"] = json!({"layout":"lead_partition_tiles_v1","books":[crate::runs::Book {
            lead: 0,
            revision: e.revision.clone(),
            entries: 1,
            metadata_sha256: file_hash(&path.join("metadata.json")).unwrap(),
            snapshot_id: meta["snapshot_id"].as_str().unwrap().to_owned(),
        }]});
        atomic(parent_path.join("metadata.json"), &manifest).unwrap();
        atomic(parent_path.join("entry.json"), &parent).unwrap();
        let mut c = Catalog::default();
        c.current.insert("hrrr".into(), parent);
        atomic(root.path().join("catalog.json"), &c).unwrap();
        crate::store::discard(root.path(), &path).unwrap();
        assert!(path.join(FILES[0]).exists());
        atomic(root.path().join("catalog.json"), &Catalog::default()).unwrap();
        crate::store::discard(root.path(), &path).unwrap();
        assert!(!path.join(FILES[0]).exists());
        binding(&shared, &meta)
            .unwrap()
            .unwrap()
            .stored(
                0,
                &mut stage::Scratch::new().unwrap(),
                &mut crate::cloud_coverage::Scratch::new().unwrap(),
            )
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
