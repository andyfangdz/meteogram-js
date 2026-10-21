use crate::runs::publish;
use crate::{
    db::Db,
    grid::{self, Lambert},
    state::*,
    store,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

#[tokio::test]
async fn preflight_checks_late_missing_chunks_and_propagates_errors() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let checked = AtomicUsize::new(0);
    let active = AtomicUsize::new(0);
    let peak = AtomicUsize::new(0);
    let keys: Vec<_> = (0..100).map(|i| format!("chunk/{i:03}")).collect();
    let report = crate::source::check_references(keys.clone(), |key| {
        checked.fetch_add(1, Ordering::Relaxed);
        let active = &active;
        let peak = &peak;
        async move {
            peak.fetch_max(
                active.fetch_add(1, Ordering::Relaxed) + 1,
                Ordering::Relaxed,
            );
            tokio::task::yield_now().await;
            active.fetch_sub(1, Ordering::Relaxed);
            Ok(key != "chunk/099")
        }
    })
    .await
    .unwrap();
    assert_eq!(checked.load(Ordering::Relaxed), 100);
    assert!((2..=16).contains(&peak.load(Ordering::Relaxed)));
    assert_eq!(report["complete"], false);
    assert_eq!(report["missing_count"], 1);
    assert_eq!(report["missing_examples"], json!(["chunk/099"]));
    let report = crate::source::check_references(keys.clone(), |_| async { Ok(true) })
        .await
        .unwrap();
    assert_eq!(report["complete"], true);
    assert_eq!(report["chunks"], 100);
    assert!(crate::source::check_references(keys, |_| async {
        anyhow::bail!("manifest unavailable")
    })
    .await
    .is_err());
}
#[test]
fn cache_pruning_keeps_uncheckpointed_maps_and_is_resumable() {
    let root = tempfile::tempdir().unwrap();
    let keys: Vec<String> = ["temperature/c/0/0", "wind/c/0/0", "temperature/c/0/1"]
        .into_iter()
        .map(String::from)
        .collect();
    for key in &keys {
        let path = crate::source::cache_path(root.path(), key);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, b"encoded map").unwrap();
    }
    assert_eq!(
        crate::source::prune_checkpointed_cache(root.path(), &keys[..2]).unwrap(),
        22
    );
    assert!(crate::source::cache_path(root.path(), &keys[2]).exists());
    assert!(!crate::source::cache_path(root.path(), &keys[0]).exists());
    assert_eq!(
        crate::source::prune_checkpointed_cache(root.path(), &keys[..2]).unwrap(),
        0
    );
}
#[test]
fn catalog_lock_drains_readers() {
    let root = tempfile::tempdir().unwrap();
    let reader = Lock::acquire(root.path(), "catalog", false, false).unwrap();
    assert!(Lock::acquire(root.path(), "catalog", true, true).is_err());
    let another = Lock::acquire(root.path(), "catalog", false, true).unwrap();
    drop(reader);
    assert!(Lock::acquire(root.path(), "catalog", true, true).is_err());
    drop(another);
    let writer = Lock::acquire(root.path(), "catalog", true, true).unwrap();
    assert!(Lock::acquire(root.path(), "catalog", false, true).is_err());
    drop(writer);
    assert!(Lock::acquire(root.path(), "catalog", true, true).is_ok());
}
fn metadata(model: &str, age: i64) -> Value {
    let init = chrono::Utc::now() - chrono::Duration::hours(age);
    json!({"model":model,"dataset":dataset(model).unwrap(),"init_time":init.format("%Y-%m-%dT%H:%M:%S").to_string(),"snapshot_id":"test_snapshot","coverage":"conus","cells":if grid_relative(model){1638757}else{24885},"spatial_shards":1,"levels":pressure_levels(model),"leads":cycle_leads(model,init.timestamp()),"variables":variables(model),"surface_variables":surface_variables(model)})
}
#[tokio::test]
async fn incremental_readiness_requires_every_field_but_not_previous_hours() {
    let hours = vec![
        (0, vec!["0/t".into(), "0/w".into()]),
        (1, vec!["1/t".into(), "1/w".into()]),
        (2, vec!["2/t".into(), "2/w".into()]),
    ];
    let counts = crate::source::ready_hours(hours.clone(), |key| async move { Ok(key != "1/w") })
        .await
        .unwrap();
    assert_eq!(counts, vec![(0, 0), (1, 1), (2, 0)]);
    assert_eq!(
        crate::source::ready_hours(hours, |_| async { Ok(true) })
            .await
            .unwrap(),
        vec![(0, 0), (1, 0), (2, 0)]
    );
}
#[test]
fn hourly_hrrr_is_complete_only_at_its_declared_horizon() {
    let mut m = metadata("hrrr", 1);
    m["dataset"] = json!(HRRR_SHORT);
    m["init_time"] = json!("2026-10-18T13:00:00");
    m["leads"] = json!((0..19).collect::<Vec<_>>());
    check_metadata(&m).unwrap();
    assert!(complete(&m).unwrap());
    m["leads"] = json!([0, 2]);
    assert!(check_metadata(&m).is_err());
    check_segment_metadata(&m).unwrap();
    assert!(!complete(&m).unwrap());
    m["init_time"] = json!("2026-10-18T12:00:00");
    assert!(check_segment_metadata(&m).is_err());
    m["dataset"] = json!(dataset("hrrr").unwrap());
    m["leads"] = json!((0..19).collect::<Vec<_>>());
    assert!(!complete(&m).unwrap());
    assert!(check_metadata(&m).is_err());
}
fn manifest_fixture(
    root: &Path,
    model: &str,
    product: &str,
    init: i64,
) -> (std::path::PathBuf, Entry) {
    let (path, mut entry) = candidate(root, model).unwrap();
    entry.owned = false;
    let mut m = metadata(model, 1);
    m["dataset"] = json!(product);
    m["leads"] = json!(product_leads(model, product));
    m["init_time"] = json!(chrono::DateTime::from_timestamp(init, 0)
        .unwrap()
        .format("%Y-%m-%dT%H:%M:%S")
        .to_string());
    m["leads"] = json!(expected_leads(&m).unwrap());
    let entries =
        m["levels"].as_array().unwrap().len() * m["variables"].as_array().unwrap().len() + 2;
    let mut books = Vec::new();
    for lead in expected_leads(&m).unwrap() {
        let (child_path, mut child) = candidate(root, model).unwrap();
        child.table = entry.table.clone();
        child.owned = false;
        let mut cm = m.clone();
        cm["leads"] = json!([lead]);
        cm["storage"] = json!({"layout":"cell_tiles_v1","encoding":crate::dictionary::ENCODING,"tile_size":256,"axes":"level_lead","packing":"uint16x8_le","element_type":"UInt128","index_type":"UInt16","codec":"delta","delta_bytes":2,"partitioned_lead":true,"lead_hour":lead,"dictionary":{"entries":entries}});
        let proof = json!({"database":entry.database,"table":entry.table,"init_time":cm["init_time"],"snapshot_id":cm["snapshot_id"],"storage":cm["storage"],"full_values_checked":true,"staged_comparison":true});
        atomic(child_path.join("entry.json"), &child).unwrap();
        atomic(child_path.join("metadata.json"), &cm).unwrap();
        atomic(child_path.join("validated.json"), &proof).unwrap();
        atomic(child_path.join("source-validation.json"), &proof).unwrap();
        books.push(crate::runs::Book {
            lead,
            revision: child.revision,
            entries,
            metadata_sha256: crate::dictionary::file_hash(&child_path.join("metadata.json"))
                .unwrap(),
            snapshot_id: cm["snapshot_id"].as_str().unwrap().to_owned(),
        });
    }
    m["storage"] = json!({"layout":"lead_partition_tiles_v1","version":1,"encoding":crate::runs::ENCODING,"tile_size":256,"axes":"level","packing":"uint16x8_le","element_type":"UInt128","index_type":"UInt16","expected_leads":m["leads"],"complete":true,"codebook_entries":entries*books.len(),"books":books});
    atomic(path.join("entry.json"), &entry).unwrap();
    atomic(path.join("metadata.json"), &m).unwrap();
    let proof = crate::runs::validate_manifest(root, &entry, &m, false).unwrap();
    atomic(path.join("validated.json"), &proof).unwrap();
    (path, entry)
}

fn staged_hour(
    path: &Path,
    mut m: Value,
    ids: &[u32],
    lat: &[f64],
    lon: &[f64],
    profiles: &[f32],
    surfaces: &[f32],
) {
    let nc = ids.len();
    let nl = m["levels"].as_array().unwrap().len();
    let nf = m["variables"].as_array().unwrap().len();
    let lead = m["leads"][0].as_u64().unwrap();
    let mut p = vec![0f32; profiles.len()];
    let mut s = vec![0f32; surfaces.len()];
    let mut fields = Vec::new();
    for f in 0..nf + 2 {
        let mut palettes = Vec::new();
        for l in 0..if f < nf { nl } else { 1 } {
            let values = (0..nc)
                .map(|cell| {
                    if f < nf {
                        profiles[(cell * nl + l) * nf + f]
                    } else {
                        surfaces[cell * 2 + f - nf]
                    }
                })
                .collect();
            let (codes, palette) = crate::dictionary::encode_spatial(values).unwrap();
            for (cell, code) in codes.into_iter().enumerate() {
                if f < nf {
                    p[(cell * nl + l) * nf + f] = code;
                } else {
                    s[cell * 2 + f - nf] = code;
                }
            }
            palettes.push(palette);
        }
        fields.push(palettes);
    }
    fs::create_dir_all(path).unwrap();
    crate::stage::save_cells(path, ids, lat, lon).unwrap();
    let info = crate::dictionary::Dictionary::new(fields)
        .save(path, &m)
        .unwrap();
    m["staging"] =
        json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
    atomic(path.join("metadata.json"), &m).unwrap();
    crate::stage::write_codes(&path.join(format!("lead-{lead:03}")), &[nc, nl, nf], &p).unwrap();
    crate::stage::write_codes(&path.join(format!("surface-{lead:03}")), &[nc, 2], &s).unwrap();
}
fn archived_fixture(root: &Path, model: &str, product: &str, init: i64) -> Entry {
    let (_, entry) = manifest_fixture(root, model, product, init);
    // Seed retained archives without selecting them as the current forecast.
    // Retention tests deliberately include old short runs below serving freshness.
    let mut catalog = catalog(root).unwrap();
    crate::runs::normalize(root, &mut catalog, Some((model, &entry))).unwrap();
    atomic(root.join("catalog.json"), &catalog).unwrap();
    entry
}
#[test]
fn retention_keeps_two_complete_runs_and_deduplicates_hrrr_long() {
    let root = tempfile::tempdir().unwrap();
    let base = (now() as i64).div_euclid(21600) * 21600 - 21600;
    let long = archived_fixture(root.path(), "hrrr", dataset("hrrr").unwrap(), base);
    archived_fixture(root.path(), "hrrr", HRRR_SHORT, base + 3600);
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.complete_runs["hrrr"].len(), 2);
    assert_eq!(crate::runs::direct(&c, false).len(), 2);
    archived_fixture(root.path(), "hrrr", HRRR_SHORT, base + 7200);
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.complete_runs["hrrr"].len(), 2);
    assert_eq!(c.hrrr_long, Some(long));
    assert_eq!(crate::runs::direct(&c, false).len(), 3);
    let next = archived_fixture(root.path(), "hrrr", dataset("hrrr").unwrap(), base + 21600);
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.hrrr_long, Some(next));
    assert_eq!(crate::runs::direct(&c, false).len(), 2);
    assert!(c.complete_runs["hrrr"].iter().all(|e| complete(
        &read_json(descriptor(root.path(), e).unwrap().join("metadata.json")).unwrap()
    )
    .unwrap()));
}
#[test]
fn an_older_complete_long_hrrr_run_is_retained_without_replacing_the_latest_short_run() {
    let root = tempfile::tempdir().unwrap();
    let mut short_init = (now() as i64).div_euclid(3600) * 3600;
    if short_init.rem_euclid(21600) == 0 {
        short_init -= 3600;
    }
    let (_, latest) = manifest_fixture(root.path(), "hrrr", HRRR_SHORT, short_init);
    publish(root.path(), "hrrr", latest.clone(), false).unwrap();
    let long_init = short_init.div_euclid(21600) * 21600;
    let (_, long) = manifest_fixture(root.path(), "hrrr", dataset("hrrr").unwrap(), long_init);
    publish(root.path(), "hrrr", long.clone(), false).unwrap();
    let catalog = catalog(root.path()).unwrap();
    assert_eq!(catalog.current["hrrr"], latest);
    assert_eq!(catalog.hrrr_long, Some(long));
    assert_eq!(catalog.complete_runs["hrrr"].len(), 2);
}
#[test]
fn lead_book_reader_enforces_remaining_run_budget_before_allocation() {
    let root = tempfile::tempdir().unwrap();
    let mut m = json!({"cells":2,"leads":[0],"levels":[1000],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
    let d = crate::dictionary::Dictionary::new(vec![vec![vec![0, 1f32.to_bits()]]; 3]);
    let info = d.save(root.path(), &m).unwrap();
    m["staging"] =
        json!({"encoding":crate::dictionary::ENCODING,"dictionary":info,"complete":true});
    assert_eq!(
        crate::dictionary::Dictionary::load_values_budget(root.path(), &m, 6)
            .unwrap()
            .entry_count()
            .unwrap(),
        6
    );
    assert!(
        crate::dictionary::Dictionary::load_values_budget(root.path(), &m, 5)
            .err()
            .unwrap()
            .to_string()
            .contains("aggregate codebook budget")
    );
}
fn incremental_value(cell: usize, level: usize, field: usize, lead: u16) -> f32 {
    if cell == 0 && level == 0 && field == 0 && lead == 0 {
        -0.
    } else {
        (cell % 17) as f32 + level as f32 * 0.25 + field as f32 * 2. + f32::from(lead) * 0.125
    }
}
#[test]
#[ignore = "requires the isolated loopback ClickHouse server"]
fn incremental_database_publication_recovers_without_duplicates_or_losing_prior_hours() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_test_incremental_{}", uuid::Uuid::new_v4().simple());
    let mut private_tables = Vec::new();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let base = now() as i64 - 3600;
        for age in [18, 12] {
            archived_fixture(
                root.path(),
                "gfs",
                dataset("gfs").unwrap(),
                base - age * 3600,
            );
        }
        let nc = 24885usize;
        let nl = 30;
        let nf = 6;
        let ys = (0..105).map(|i| 24. + i as f64 * 0.25).collect::<Vec<_>>();
        let xs = (0..237)
            .map(|i| -125. + i as f64 * 0.25)
            .collect::<Vec<_>>();
        let ids = (0..nc as u32).collect::<Vec<_>>();
        let lat = (0..nc).map(|i| ys[i / 237]).collect::<Vec<_>>();
        let lon = (0..nc).map(|i| xs[i % 237]).collect::<Vec<_>>();
        let mut latest = None;
        for lead in [0u16, 2] {
            let raw = root.path().join(format!("raw{lead}"));
            fs::create_dir(&raw).unwrap();
            let mut m = metadata("gfs", 1);
            m["leads"] = json!([lead]);
            m["init_time"] = json!(chrono::DateTime::from_timestamp(base, 0)
                .unwrap()
                .format("%Y-%m-%dT%H:%M:%S")
                .to_string());
            m["snapshot_id"] = json!(format!("test_snapshot_{lead}"));
            m["grid"] = json!({"kind":"latlon","latitude":ys,"longitude":xs,"native_cells":nc,"conus_cells":nc});
            m["units"] = json!({});
            let p = (0..nc * nl * nf)
                .map(|i| incremental_value(i / (nl * nf), (i / nf) % nl, i % nf, lead))
                .collect::<Vec<_>>();
            let surfaces = (0..nc * 2)
                .map(|i| incremental_value(i / 2, 0, nf + i % 2, lead))
                .collect::<Vec<_>>();
            let (path, entry) = candidate(&root.path().join("incoming"), "gfs").unwrap();
            staged_hour(&path, m, &ids, &lat, &lon, &p, &surfaces);
            private_tables.push(entry.table.clone());
            store::load_lead(root.path(), &path, &entry.table, lead).unwrap();
            if lead == 2 {
                let before = fs::read(root.path().join("catalog.json")).unwrap();
                assert!(crate::incremental::attach_lead(
                    root.path(),
                    "gfs",
                    &path,
                    &entry,
                    &table,
                    Some("after_attach")
                )
                .is_err());
                assert_eq!(fs::read(root.path().join("catalog.json")).unwrap(), before);
                let (status, v) = crate::api::test_forecast(
                    root.path(),
                    [("model", "gfs"), ("lat", "41"), ("lon", "-74")]
                        .into_iter()
                        .map(|(k, v)| (k.to_owned(), v.to_owned()))
                        .collect(),
                );
                assert_eq!(status, axum::http::StatusCode::OK);
                assert_eq!(v["available_leads"], json!([0]));
                assert_eq!(v["rows"].as_array().unwrap().len(), 1);
                assert_eq!(v["forecast_complete"], false);
            }
            let e =
                crate::incremental::attach_lead(root.path(), "gfs", &path, &entry, &table, None)
                    .unwrap();
            let duplicate =
                crate::incremental::attach_lead(root.path(), "gfs", &path, &entry, &table, None)
                    .unwrap();
            assert_eq!(e, duplicate);
            assert_eq!(catalog(root.path()).unwrap().complete_runs["gfs"].len(), 2);
            store::discard(root.path(), &path).unwrap();
            latest = Some(e);
        }
        let e = latest.unwrap();
        store::collect(root.path(), 0.).unwrap();
        let m = read_json(descriptor(root.path(), &e).unwrap().join("metadata.json")).unwrap();
        store::validate(
            &descriptor(root.path(), &e).unwrap(),
            &e.table,
            DATABASE,
            false,
        )
        .unwrap();
        let reader = crate::runs::Reader::load(root.path(), &e, &m).unwrap();
        for i in [0, nc / 2, nc - 1, 16320] {
            let (rows, _) = reader.profile(&db, &e, &m, ids[i]).unwrap();
            assert_eq!(rows.len(), 2);
            for (t, lead) in [0u16, 2].iter().enumerate() {
                for field in 0..nf {
                    for (level, value) in rows[t][field + 1].as_array().unwrap().iter().enumerate()
                    {
                        assert_eq!(
                            (value.as_f64().unwrap() as f32).to_bits(),
                            incremental_value(i, level, field, *lead).to_bits()
                        );
                    }
                }
                for field in 0..2 {
                    assert_eq!(
                        (rows[t][nf + field + 1].as_f64().unwrap() as f32).to_bits(),
                        incremental_value(i, 0, nf + field, *lead).to_bits()
                    );
                }
            }
        }
        assert_eq!(
            db.scalar(&format!("SELECT count() FROM meteogram_native.{table}"))
                .unwrap(),
            2 * nc.div_ceil(256) as u64
        );
        let (status, v) = crate::api::test_forecast(
            root.path(),
            [("model", "gfs"), ("lat", "41"), ("lon", "-74")]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
        );
        assert_eq!(status, axum::http::StatusCode::OK);
        assert_eq!(v["available_leads"], json!([0, 2]));
        assert!(v["missing_leads"].as_array().unwrap().contains(&json!(1)));
        assert_eq!(v["lead_sources"].as_array().unwrap().len(), 2);
        // A rollback can select a partial run. Neither
        // complete-only selector may silently return one without an archive.
        let mut partial_catalog = catalog(root.path()).unwrap();
        partial_catalog.complete_runs.clear();
        partial_catalog.previous.insert("gfs".to_owned(), e);
        atomic(root.path().join("catalog.json"), &partial_catalog).unwrap();
        for run in ["latest_complete", "previous_complete"] {
            let (status, _) = crate::api::test_forecast(
                root.path(),
                [
                    ("model", "gfs"),
                    ("lat", "41"),
                    ("lon", "-74"),
                    ("run", run),
                ]
                .into_iter()
                .map(|(k, v)| (k.to_owned(), v.to_owned()))
                .collect(),
            );
            assert_eq!(status, axum::http::StatusCode::SERVICE_UNAVAILABLE);
        }
    }));
    for name in private_tables.into_iter().chain([table]) {
        let _ = db.command(&format!(
            "DROP TABLE IF EXISTS meteogram_native.{name} SYNC"
        ));
    }
    assert!(result.is_ok());
}
#[test]
fn rrfs_has_its_own_complete_horizon_and_freshness_policy() {
    assert_eq!(leads("rrfs"), (0..=84).collect::<Vec<_>>());
    assert_eq!(
        dataset("rrfs").unwrap(),
        "noaa-rrfs-forecast-84-hour-virtual"
    );
    assert!(grid_relative("rrfs"));
    assert!(grid_relative("hrrr"));
    assert!(!grid_relative("gfs"));
    assert!(!grid_relative("aifs"));
    assert_eq!(freshness(&metadata("rrfs", 23)).unwrap()["available"], true);
    assert_eq!(
        freshness(&metadata("rrfs", 25)).unwrap()["available"],
        false
    );
    let mut incomplete = metadata("rrfs", 6);
    incomplete["leads"] = json!(leads("hrrr"));
    assert!(check_metadata(&incomplete).is_err());
    incomplete["leads"] = json!([0, 48]);
    assert_eq!(freshness(&incomplete).unwrap()["available"], false);
}
#[test]
fn regional_manifest_caches_are_bounded_without_retaining_weather_payloads() {
    for model in ["hrrr", "rrfs"] {
        let cache = crate::source::initial_config(model).caching.unwrap();
        assert_eq!(cache.num_chunk_refs, Some(8_000_000));
        assert_eq!(cache.num_bytes_chunks, Some(0));
    }
    for model in ["gfs", "aifs"] {
        assert!(crate::source::initial_config(model).caching.is_none());
    }
}
fn candidate_fixture(root: &Path, age: i64) -> (std::path::PathBuf, Entry) {
    manifest_fixture(
        root,
        "gfs",
        dataset("gfs").unwrap(),
        (now() as i64) - age * 3600,
    )
}
#[test]
fn publication_requires_validation_and_advancing_identity() {
    let root = tempfile::tempdir().unwrap();
    let (p, e) = candidate_fixture(root.path(), 12);
    let marker = fs::read(p.join("validated.json")).unwrap();
    fs::remove_file(p.join("validated.json")).unwrap();
    assert!(publish(root.path(), "gfs", e.clone(), false).is_err());
    fs::write(p.join("validated.json"), marker).unwrap();
    publish(root.path(), "gfs", e.clone(), false).unwrap();
    assert!(publish(root.path(), "gfs", e, false).is_err());
    let (_, new) = candidate_fixture(root.path(), 6);
    let c = publish(root.path(), "gfs", new.clone(), false).unwrap();
    assert_eq!(c.generation, 2);
    let prior = c.previous["gfs"].clone();
    publish(root.path(), "gfs", prior, true).unwrap();
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.previous["gfs"].revision, new.revision);
}
#[test]
fn metadata_mutation_never_publishes() {
    let root = tempfile::tempdir().unwrap();
    for key in ["snapshot_id", "init_time", "storage"] {
        let (p, e) = candidate_fixture(root.path(), 6);
        let mut m = read_json(p.join("metadata.json")).unwrap();
        m[key] = if key == "storage" {
            json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":"float32_bits","codec":"t64"})
        } else {
            json!("tampered")
        };
        atomic(p.join("metadata.json"), &m).unwrap();
        assert!(publish(root.path(), "gfs", e, false).is_err());
    }
    assert_eq!(catalog(root.path()).unwrap().generation, 0);
}
#[test]
fn freshness_rejects_future_stale_and_short_horizon() {
    for model in MODELS {
        assert_eq!(freshness(&metadata(model, 6)).unwrap()["available"], true);
        assert_eq!(freshness(&metadata(model, -2)).unwrap()["available"], false);
        assert_eq!(freshness(&metadata(model, 49)).unwrap()["available"], false);
    }
    let mut m = metadata("gfs", 6);
    m["leads"] = json!([0, 6]);
    assert_eq!(freshness(&m).unwrap()["available"], false);
}
#[test]
fn full_source_schema_has_no_silent_drift() {
    for model in MODELS {
        let m = metadata(model, 6);
        check_metadata(&m).unwrap();
        for key in ["levels", "leads", "variables"] {
            let mut bad = m.clone();
            bad[key].as_array_mut().unwrap().pop();
            assert!(check_metadata(&bad).is_err());
        }
        let mut bad = m;
        bad["cells"] = json!(3);
        assert!(check_metadata(&bad).is_err());
    }
}
#[test]
fn unsupported_storage_formats_fail_closed() {
    let mut m = metadata("gfs", 1);
    for storage in [
        json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":"float32_bits","codec":"t64"}),
        json!({"layout":"cell_profile_v1","axes":"level_lead","encoding":crate::dictionary::ENCODING,"index_type":"UInt32","codec":"t64"}),
        json!({"layout":"cell_tiles_v1","axes":"level_lead","encoding":crate::dictionary::ENCODING,"index_type":"UInt16","tile_size":256,"packing":"uint16x8_le","element_type":"UInt128","codec":"delta","delta_bytes":2}),
    ] {
        m["storage"] = storage;
        assert!(check_metadata(&m).is_err());
    }
    let root = tempfile::tempdir().unwrap();
    assert!(
        crate::stage::BlockWriter::new(&root.path().join("weather"), &[1], "uint32_le").is_err()
    );
}
#[test]
fn unsafe_identifiers_and_revisions_are_rejected() {
    for s in ["", "../bad", "a;DROP", "Camel", "a.b", "x-y", "1start"] {
        assert!(ident(s).is_err());
    }
    assert!(ident("hrrr_20261018_ab12").is_ok());
}
#[test]
fn spherical_lambert_roundtrip_matches_known_origin() {
    let p = Lambert::new(6371229., 38.5, -97.5, 38.5, 38.5, 0., 0.).unwrap();
    assert!(p.forward(38.5, -97.5).0.abs() < 1e-8);
    assert!(p.forward(38.5, -97.5).1.abs() < 1e-8);
    assert!(p.convergence_degrees(-97.5).abs() < 1e-12);
    assert!((p.convergence_degrees(-74.283767) - 14.452).abs() < 0.02);
    for (lat, lon) in [(24., -125.), (50., -66.), (41., -74.), (37., -120.)] {
        let (x, y) = p.forward(lat, lon);
        let (la, lo) = p.inverse(x, y);
        assert!((la - lat).abs() < 1e-10);
        assert!((lo - lon).abs() < 1e-10);
        // A true north displacement projects as (-sin(theta),cos(theta)).
        // This checks the sign against the projection, independently of the
        // wind adapter's component rotation.
        let (nx, ny) = p.forward(lat + 0.00001, lon);
        let bearing = (-(nx - x)).atan2(ny - y).to_degrees();
        assert!((bearing - p.convergence_degrees(lon)).abs() < 1e-7);
    }
}
#[test]
fn coordinate_resolver_rejects_just_outside_conus() {
    let m =
        json!({"grid":{"kind":"latlon","latitude":[50.,40.,24.],"longitude":[-125.,-74.,-66.]}});
    let ids = (0..9).collect::<Vec<_>>();
    for (lat, lon) in [
        (23.999999, -74.),
        (50.000001, -74.),
        (40., -125.000001),
        (40., -65.999999),
    ] {
        assert!(grid::resolve(&m, &ids, lat, lon).is_err());
    }
    assert_eq!(grid::resolve(&m, &ids, 40., -74.).unwrap(), (4, 4));
}

#[test]
#[ignore = "requires the isolated loopback ClickHouse server"]
fn hourly_tiles_roundtrip_float_bits_and_reject_unselected_corruption() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_hourly_{}", uuid::Uuid::new_v4().simple());
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let cells = 1030;
        let lead = 7u16;
        let nl = 2;
        let nf = 2;
        let ids = (0..cells as u32).map(|i| i * 7 + 3).collect::<Vec<_>>();
        let mut m = metadata("gfs", 1);
        m["cells"] = json!(cells);
        m["leads"] = json!([lead]);
        m["levels"] = json!([1000, 500]);
        m["variables"] = json!(["temperature", "geopotential_height"]);
        let bits = [0, 0x80000000, 1, 0x7f7fffff, 0xff7fffff, 0x3f812345];
        let profiles = (0..cells * nl * nf)
            .map(|i| f32::from_bits(bits[(i + i / (nl * nf)) % bits.len()]))
            .collect::<Vec<_>>();
        let surfaces = (0..cells * 2)
            .map(|i| f32::from_bits(bits[(i + i / 2) % bits.len()]))
            .collect::<Vec<_>>();
        staged_hour(
            root.path(),
            m,
            &ids,
            &vec![41.; cells],
            &vec![-74.; cells],
            &profiles,
            &surfaces,
        );
        let proof = store::load_lead(root.path(), root.path(), &table, lead).unwrap();
        assert_eq!(proof["full_values_checked"], true);
        assert_eq!(proof["staged_comparison"], true);
        let m = read_json(root.path().join("metadata.json")).unwrap();
        let dictionary = crate::dictionary::Dictionary::load(root.path(), &m).unwrap();
        let books = std::collections::BTreeMap::from([(lead, &dictionary)]);
        for i in [0, 255, 256, cells - 1] {
            let (rows, _) = db
                .profile_leads(DATABASE, &table, &m, ids[i], i, &books)
                .unwrap();
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0], lead);
            for f in 0..nf {
                for l in 0..nl {
                    assert_eq!(
                        (rows[0][f + 1][l].as_f64().unwrap() as f32).to_bits(),
                        profiles[(i * nl + l) * nf + f].to_bits()
                    );
                }
            }
            for f in 0..2 {
                assert_eq!(
                    (rows[0][nf + f + 1].as_f64().unwrap() as f32).to_bits(),
                    surfaces[i * 2 + f].to_bits()
                );
            }
        }
        store::validate(root.path(), &table, DATABASE, true).unwrap();
        // The requested cell is unchanged; a different word still must fail the full audit.
        db.command(&format!("ALTER TABLE {DATABASE}.{table} UPDATE temperature=arrayMap((x,i)->if(i=32,toUInt128(65535),x),temperature,arrayEnumerate(temperature)) WHERE tile_id=0 SETTINGS mutations_sync=2")).unwrap();
        assert!(db
            .profile_leads(DATABASE, &table, &m, ids[0], 0, &books)
            .is_ok());
        assert!(store::validate(root.path(), &table, DATABASE, false).is_err());
        assert!(!root.path().join("validated.json").exists());
        // Projection must also retain the original width check for an unused final word.
        db.command(&format!("ALTER TABLE {DATABASE}.{table} UPDATE temperature=arrayPopBack(temperature) WHERE tile_id=0 SETTINGS mutations_sync=2")).unwrap();
        assert!(db
            .profile_leads(DATABASE, &table, &m, ids[0], 0, &books)
            .is_err());
    }));
    let _ = db.command(&format!("DROP TABLE IF EXISTS {DATABASE}.{table} SYNC"));
    assert!(result.is_ok());
}

#[test]
#[ignore = "requires the isolated loopback ClickHouse server"]
fn parallel_load_failure_drains_insert_workers_before_returning() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_failed_load_{}", uuid::Uuid::new_v4().simple());
    let cells = 5 * 4096 + 3;
    let ids = (0..cells as u32).collect::<Vec<_>>();
    let mut m = metadata("gfs", 1);
    m["cells"] = json!(cells);
    m["levels"] = json!([1000]);
    m["variables"] = json!(["temperature"]);
    m["leads"] = json!([0]);
    staged_hour(
        root.path(),
        m,
        &ids,
        &vec![41.; cells],
        &vec![-74.; cells],
        &vec![1.; cells],
        &vec![1.; cells * 2],
    );
    atomic(
        root.path().join("validated.json"),
        &json!({"table":"stale_prior_attempt"}),
    )
    .unwrap();
    let index = read_json(root.path().join("lead-000.stage.json")).unwrap();
    let offset = index["blocks"][2]["offset"].as_u64().unwrap() as usize;
    let data = root.path().join("lead-000.stage.zst");
    let mut bytes = fs::read(&data).unwrap();
    bytes[offset + 20] ^= 1;
    fs::write(data, bytes).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(
            store::load_lead_with_workers(root.path(), root.path(), &table, 0, Some(4)).is_err()
        );
        assert!(!root.path().join("validated.json").exists());
        assert_eq!(db.scalar(&format!("SELECT count() FROM system.processes WHERE startsWith(query,'INSERT INTO {DATABASE}.{table} ')")).unwrap(),0);
    }));
    let _ = db.command(&format!("DROP TABLE IF EXISTS {DATABASE}.{table} SYNC"));
    assert!(result.is_ok());
}

#[test]
fn existing_float_pressure_metadata_is_accepted() {
    let mut m = metadata("gfs", 6);
    m["levels"] = json!((275..1001)
        .step_by(25)
        .rev()
        .map(|v| v as f64)
        .collect::<Vec<_>>());
    check_metadata(&m).unwrap();
}
#[test]
fn validation_without_the_current_storage_binding_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let (p, e) = candidate_fixture(root.path(), 6);
    let mut v = read_json(p.join("validated.json")).unwrap();
    v["storage"] = Value::Null;
    atomic(p.join("validated.json"), &v).unwrap();
    assert!(publish(root.path(), "gfs", e, false).is_err());
}
#[test]
fn zarr_temperature_decode_uses_float64_before_float32() {
    let values = [273.15f64, 299.123456789, 100., -0.];
    let raw: Vec<u8> = values.iter().flat_map(|v| v.to_le_bytes()).collect();
    let b = zstd::encode_all(raw.as_slice(), 1).unwrap();
    let meta = json!({"data_type":"float64","codecs":[{"name":"scale_offset","configuration":{"offset":-273.15}},{"name":"bytes","configuration":{"endian":"little"}},{"name":"zstd"}]});
    let got = crate::source::decode(&meta, &b).unwrap();
    for (a, b) in got.iter().zip(values) {
        assert_eq!((*a as f32).to_bits(), ((b - 273.15) as f32).to_bits());
    }
}
#[test]
#[ignore = "requires the isolated loopback ClickHouse server"]
fn garbage_collection_respects_ownership_and_live_references() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let prefix = format!("rust_gc_{}", uuid::Uuid::new_v4().simple());
    let mut entries = vec![];
    for (name, owned) in [("owned", true), ("borrowed", false), ("live", true)] {
        let table = format!("{prefix}_{name}");
        db.command(&format!(
            "CREATE TABLE meteogram_native.{table} (x UInt32) ENGINE=Memory"
        ))
        .unwrap();
        let e = Entry {
            revision: table.clone(),
            database: DATABASE.into(),
            table,
            owned,
            retired_at: Some(0.),
        };
        let p = descriptor(root.path(), &e).unwrap();
        fs::create_dir_all(&p).unwrap();
        atomic(
            p.join("metadata.json"),
            &json!({"storage":{"layout":"lead_partition_tiles_v1","books":[]}}),
        )
        .unwrap();
        entries.push(e);
    }
    let mut c = Catalog::default();
    c.current.insert("hrrr".into(), entries[2].clone());
    c.retired = entries.clone();
    atomic(root.path().join("catalog.json"), &c).unwrap();
    store::collect(root.path(), 0.).unwrap();
    assert_eq!(
        db.scalar(&format!(
            "EXISTS TABLE meteogram_native.{}",
            entries[0].table
        ))
        .unwrap(),
        0
    );
    assert_eq!(
        db.scalar(&format!(
            "EXISTS TABLE meteogram_native.{}",
            entries[1].table
        ))
        .unwrap(),
        1
    );
    assert_eq!(
        db.scalar(&format!(
            "EXISTS TABLE meteogram_native.{}",
            entries[2].table
        ))
        .unwrap(),
        1
    );
    for e in entries {
        db.command(&format!(
            "DROP TABLE IF EXISTS meteogram_native.{} SYNC",
            e.table
        ))
        .unwrap();
    }
}

#[test]
fn public_s3_never_requests_host_credentials() {
    use icechunk::{
        config::{Credentials, ObjectStoreConfig, S3Credentials, S3Options},
        virtual_chunks::VirtualChunkContainer,
        RepositoryConfig,
    };
    let mut c = RepositoryConfig::default();
    for p in ["s3://public/", "s3://other/"] {
        c.set_virtual_chunk_container(
            VirtualChunkContainer::new(p.to_owned(), ObjectStoreConfig::S3(S3Options::default()))
                .unwrap(),
        )
        .unwrap();
    }
    let creds = std::collections::HashMap::from([(
        "s3://public/".to_owned(),
        Some(Credentials::S3(S3Credentials::Anonymous)),
    )]);
    crate::source::public_config(&mut c, &creds);
    for (p, v) in c.virtual_chunk_containers.unwrap() {
        let ObjectStoreConfig::S3(opts) = v.store else {
            panic!("S3")
        };
        assert_eq!(opts.anonymous, p == "s3://public/");
    }
}

#[test]
fn old_failure_evidence_expires_but_published_revisions_remain() {
    let root = tempfile::tempdir().unwrap();
    let (p, e) = candidate_fixture(root.path(), 6);
    atomic(p.join("discarded.json"), &json!({"time":0})).unwrap();
    let mut c = Catalog::default();
    c.current.insert("gfs".into(), e);
    atomic(root.path().join("catalog.json"), &c).unwrap();
    store::discard(root.path(), &p).unwrap();
    assert!(p.exists());
    let (old, _) = candidate_fixture(root.path(), 6);
    atomic(old.join("discarded.json"), &json!({"time":0})).unwrap();
    store::discard(root.path(), &old).unwrap();
    assert!(!old.exists());
}
#[test]
fn bulk_cleanup_preserves_live_children_and_shared_tables() {
    let root = tempfile::tempdir().unwrap();
    let (live, entry) = candidate_fixture(root.path(), 6);
    let mut c = Catalog::default();
    c.current.insert("gfs".into(), entry.clone());
    atomic(root.path().join("catalog.json"), &c).unwrap();
    let references = crate::runs::references(root.path(), &c, true).unwrap();
    // Expired evidence must not override live parent/child references.
    for e in &references {
        atomic(
            descriptor(root.path(), e).unwrap().join("discarded.json"),
            &json!({"time":0}),
        )
        .unwrap();
    }
    for i in 0..256 {
        let p = root.path().join("revisions").join(format!("expired_{i}"));
        atomic(p.join("discarded.json"), &json!({"time":0})).unwrap();
    }
    let orphan = root.path().join("revisions/orphan");
    let mut alias = entry.clone();
    alias.revision = "orphan".into();
    alias.owned = true;
    atomic(orphan.join("entry.json"), &alias).unwrap();
    fs::write(orphan.join("values.dict.zst"), b"unused").unwrap();
    let count = store::sweep_unreferenced(root.path()).unwrap();
    assert_eq!(count, references.len() + 257);
    assert!(live.exists());
    for e in references {
        assert!(descriptor(root.path(), &e).unwrap().exists());
    }
    assert!(!root.path().join("revisions/expired_0").exists());
    assert!(!root.path().join("revisions/expired_255").exists());
    assert!(orphan.join("discarded.json").exists());
    assert!(!orphan.join("values.dict.zst").exists());
}

#[test]
fn unknown_catalog_version_is_rejected() {
    let root = tempfile::tempdir().unwrap();
    let c = Catalog {
        version: 2,
        ..Catalog::default()
    };
    atomic(root.path().join("catalog.json"), &c).unwrap();
    assert!(catalog(root.path()).is_err());
}

#[test]
fn ifs_short_cycles_do_not_replace_the_complete_long_base() {
    let root = tempfile::tempdir().unwrap();
    let base = (now() as i64).div_euclid(43200) * 43200 - 86400;
    let long = archived_fixture(root.path(), "ifs", dataset("ifs").unwrap(), base);
    archived_fixture(root.path(), "ifs", dataset("ifs").unwrap(), base + 21600);
    archived_fixture(root.path(), "ifs", dataset("ifs").unwrap(), base + 64800);
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.complete_runs["ifs"].len(), 2);
    assert_eq!(c.long_runs["ifs"], long);
    assert_eq!(crate::runs::direct(&c, false).len(), 3);
    let next = archived_fixture(root.path(), "ifs", dataset("ifs").unwrap(), base + 86400);
    let c = catalog(root.path()).unwrap();
    assert_eq!(c.long_runs["ifs"], next);
    assert_eq!(crate::runs::direct(&c, false).len(), 2);
}
#[test]
fn new_global_models_preserve_native_leads_levels_and_surface_availability() {
    assert_eq!(cycle_leads("ifs", 0).len(), 55);
    assert_eq!(cycle_leads("ifs", 21600).len(), 49);
    assert_eq!(cycle_leads("ifs", 43200).last(), Some(&180));
    assert_eq!(cycle_leads("ifs", 64800).last(), Some(&144));
    assert_eq!(leads("aigfs").len(), 31);
    assert_eq!(surface_variables("aigfs"), vec!["temperature_2m"]);
    let mut m = metadata("aigfs", 6);
    check_metadata(&m).unwrap();
    m["surface_variables"] = json!(["temperature_2m", "dew_point_temperature_2m"]);
    assert!(check_metadata(&m).is_err());
}
