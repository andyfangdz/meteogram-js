use crate::{
    db::Db,
    grid::{self, Lambert},
    pack,
    state::*,
    store,
};
use serde_json::{json, Value};
use std::{fs, path::Path};

#[test]
fn simd_preserves_every_float_bit_including_tails() {
    for f in [2, 5, 6] {
        for l in [1, 3, 4, 8, 30] {
            for t in [1, 3, 4, 7, 49, 141] {
                let c = 35;
                let patterns = [
                    0u32, 0x80000000, 1, 0x007fffff, 0x7f7fffff, 0xff7fffff, 0x3f812345,
                ];
                let source: Vec<Vec<f32>> = (0..t)
                    .map(|time| {
                        (0..c * l * f)
                            .map(|i| f32::from_bits(patterns[(i + time) % patterns.len()]))
                            .collect()
                    })
                    .collect();
                let pointers: Vec<_> = source.iter().map(|v| v.as_ptr()).collect();
                let mut output = vec![0f32; c * l * f * t];
                assert_eq!(
                    unsafe { pack::pack_neon(pointers.as_ptr(), output.as_mut_ptr(), c, l, f, t) },
                    0
                );
                for field in 0..f {
                    for cell in 0..c {
                        for level in 0..l {
                            for time in 0..t {
                                assert_eq!(
                                    output[((field * c + cell) * l + level) * t + time].to_bits(),
                                    source[time][(cell * l + level) * f + field].to_bits()
                                );
                            }
                        }
                    }
                }
            }
        }
    }
}
#[test]
fn simd_rejects_nonfinite_in_all_tail_positions() {
    for f in [2, 5, 6] {
        for t in [1, 4, 7] {
            for bits in [0x7f800000, 0xff800000, 0x7fc00000] {
                for at in [0, 7, 17] {
                    let mut source = vec![vec![1f32; 3 * 3 * f]; t];
                    source[t - 1][at] = f32::from_bits(bits);
                    let pointers: Vec<_> = source.iter().map(|v| v.as_ptr()).collect();
                    let mut output = vec![0f32; 3 * 3 * f * t];
                    assert_ne!(
                        unsafe {
                            pack::pack_neon(pointers.as_ptr(), output.as_mut_ptr(), 3, 3, f, t)
                        },
                        0
                    );
                }
            }
        }
    }
}
#[test]
fn simd_rejects_nonfinite_across_cell_tiles() {
    for cell in [0, 15, 16, 31, 32, 34] {
        for time in [0, 3, 4, 48] {
            let (c, l, f, t) = (35, 30, 6, 49);
            let mut source = vec![vec![1f32; c * l * f]; t];
            source[time][(cell * l + 29) * f + 5] = f32::NAN;
            let pointers: Vec<_> = source.iter().map(|v| v.as_ptr()).collect();
            let mut output = vec![0f32; c * l * f * t];
            assert_ne!(
                unsafe { pack::pack_neon(pointers.as_ptr(), output.as_mut_ptr(), c, l, f, t) },
                0
            );
        }
    }
}
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
fn numpy_cells_roundtrip_without_python() {
    let root = tempfile::tempdir().unwrap();
    let ids = [0, 31, u32::MAX];
    let lat = [24., 41., 50.];
    let lon = [-125., -74., -66.];
    save_cells(&root.path().join("cells.npz"), &ids, &lat, &lon).unwrap();
    let a = load_npz(&root.path().join("cells.npz")).unwrap();
    assert_eq!(
        a["cell_id"].bytes,
        ids.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()
    );
    assert_eq!(
        a["latitude"].bytes,
        lat.iter().flat_map(|v| v.to_le_bytes()).collect::<Vec<_>>()
    );
}
#[test]
fn malformed_npy_is_an_error() {
    assert!(parse_npy(b"bad").is_err());
    let mut b = npy_bytes("<f4", &[2, 3], &[0; 24]);
    b.truncate(12);
    assert!(parse_npy(&b).is_err());
    let b = b"\x93NUMPY\x01\x00\xff\xff";
    assert!(parse_npy(b).is_err());
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
    json!({"model":model,"dataset":dataset(model).unwrap(),"init_time":init.format("%Y-%m-%dT%H:%M:%S").to_string(),"snapshot_id":"test_snapshot","coverage":"conus","cells":if model=="hrrr"{1638757}else{24885},"spatial_shards":1,"levels":if model=="aifs"{vec![1000,925,850,700,600,500,400,300]}else{(275..1001).step_by(25).rev().collect()},"leads":leads(model),"variables":variables(model),"surface_variables":["temperature_2m","dew_point_temperature_2m"]})
}
fn candidate_fixture(root: &Path, age: i64) -> (std::path::PathBuf, Entry) {
    let (p, e) = candidate(root, "gfs").unwrap();
    let m = metadata("gfs", age);
    atomic(p.join("metadata.json"), &m).unwrap();
    atomic(p.join("validated.json"),&json!({"table":e.table,"database":e.database,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"]})).unwrap();
    (p, e)
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
    for (lat, lon) in [(24., -125.), (50., -66.), (41., -74.), (37., -120.)] {
        let (x, y) = p.forward(lat, lon);
        let (la, lo) = p.inverse(x, y);
        assert!((la - lat).abs() < 1e-10);
        assert!((lo - lon).abs() < 1e-10);
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
fn database_native_roundtrip_and_corruption_gate() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_test_{}", uuid::Uuid::new_v4().simple());
    // Cross 4,096-cell staging/raw-load and 8,192-cell UInt16-load boundaries,
    // including a short final batch in both representations.
    let cells = 8197;
    let ids: Vec<_> = (1..=cells as u32).map(|i| i * 11).collect();
    save_cells(
        &root.path().join("cells.npz"),
        &ids,
        &vec![41.; cells],
        &vec![-74.; cells],
    )
    .unwrap();
    let m = json!({"model":"hrrr","init_time":"2026-10-17T18:00:00","snapshot_id":"test","coverage":"conus","cells":cells,"levels":[1000,500],"leads":[0,6,12],"variables":["temperature","wind_u"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
    atomic(root.path().join("metadata.json"), &m).unwrap();
    let bits = [0u32, 0x80000000, 1, 0x7f7fffff, 0xff7fffff, 0x3f812345];
    for (t, lead) in [0, 6, 12].iter().enumerate() {
        let p: Vec<u8> = (0..cells * 4)
            .flat_map(|i| bits[(i + t) % bits.len()].to_le_bytes())
            .collect();
        let g: Vec<u8> = (0..cells * 2)
            .flat_map(|i| bits[(i + t) % bits.len()].to_le_bytes())
            .collect();
        fs::write(
            root.path().join(format!("lead-{lead:03}.npy")),
            npy_bytes("<f4", &[cells, 2, 2], &p),
        )
        .unwrap();
        fs::write(
            root.path().join(format!("surface-{lead:03}.npy")),
            npy_bytes("<f4", &[cells, 2], &g),
        )
        .unwrap();
    }
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let compressed = root.path().join("compressed");
        crate::stage::convert(root.path(), &compressed).unwrap();
        let v = store::load_with_workers(root.path(), &compressed, &table, Some(4)).unwrap();
        fs::copy(
            compressed.join("metadata.json"),
            root.path().join("metadata.json"),
        )
        .unwrap();
        assert_eq!(v["rows"], cells * 3);
        assert_eq!(v["storage_rows"], cells);
        let clone = format!("{table}_clone");
        db.command(&format!(
            "CREATE TABLE meteogram_native.{clone} CLONE AS meteogram_native.{table}"
        ))
        .unwrap();
        db.command(&format!("TRUNCATE TABLE meteogram_native.{table}"))
            .unwrap();
        store::validate(root.path(), &clone, "meteogram_native", true).unwrap();
        store::validate(&compressed, &clone, "meteogram_native", true).unwrap();
        let loaded_metadata = read_json(compressed.join("metadata.json")).unwrap();
        let coded_dir = root.path().join("dictionary");
        let recoded_raw = root.path().join("raw_again");
        crate::prepare::from_store(
            &compressed,
            DATABASE,
            &clone,
            &coded_dir,
            Some(&recoded_raw),
        )
        .unwrap();
        let dict_table = format!("{table}_dict");
        let coded =
            store::load_with_workers(root.path(), &coded_dir, &dict_table, Some(4)).unwrap();
        assert_eq!(coded["storage"]["encoding"], crate::dictionary::ENCODING);
        assert_eq!(coded["storage"]["index_type"], "UInt16");
        assert_eq!(coded["storage"]["layout"], "cell_tiles_v1");
        assert_eq!(coded["storage_rows"], cells.div_ceil(crate::packed::CELLS));
        store::validate(&coded_dir, &dict_table, DATABASE, true).unwrap();
        db.command(&format!("DROP TABLE {DATABASE}.{dict_table} SYNC"))
            .unwrap();
        let sorted_dir = root.path().join("sorted_dictionary");
        crate::prepare::sorted_stage(&coded_dir, &sorted_dir).unwrap();
        let coded_dir = sorted_dir;
        store::load_with_workers(root.path(), &coded_dir, &dict_table, Some(4)).unwrap();
        store::validate(&coded_dir, &dict_table, DATABASE, true).unwrap();
        let coded_meta = read_json(coded_dir.join("metadata.json")).unwrap();
        let dictionary = crate::dictionary::Dictionary::load(&coded_dir, &coded_meta)
            .unwrap()
            .unwrap();
        for index in (0..8).chain([4096, cells - 1]) {
            let original = db
                .profile(DATABASE, &clone, &loaded_metadata, ids[index])
                .unwrap()
                .0;
            let restored = db
                .profile_with_dictionary(
                    DATABASE,
                    &dict_table,
                    &coded_meta,
                    ids[index],
                    Some(&dictionary),
                )
                .unwrap()
                .0;
            assert_eq!(original, restored);
        }
        let exported = root.path().join("packed_export");
        let raw_export = root.path().join("packed_raw");
        crate::prepare::from_store(
            &coded_dir,
            DATABASE,
            &dict_table,
            &exported,
            Some(&raw_export),
        )
        .unwrap();
        let mut decoded = vec![0f32; cells * 4];
        crate::stage::Array::open(&raw_export.join("lead-000.npy"), &[cells, 2, 2])
            .unwrap()
            .read_into(0, &mut decoded)
            .unwrap();
        assert!(decoded
            .iter()
            .enumerate()
            .all(|(i, v)| v.to_bits() == bits[i % bits.len()]));
        let book = coded_dir.join("values.dict.zst");
        let pristine = fs::read(&book).unwrap();
        let mut corrupt = pristine.clone();
        corrupt[10] ^= 1;
        fs::write(&book, corrupt).unwrap();
        assert!(crate::dictionary::Dictionary::load(&coded_dir, &coded_meta).is_err());
        fs::write(&book, pristine).unwrap();
        let invalid = 65535u128 << 112;
        db.command(&format!("ALTER TABLE {DATABASE}.{dict_table} UPDATE temperature=arrayConcat([toUInt128('{invalid}')],arrayWithConstant({},toUInt128(0))) WHERE tile_id=0 SETTINGS mutations_sync=2",6*crate::packed::GROUPS-1)).unwrap();
        assert!(store::validate(&coded_dir, &dict_table, DATABASE, false).is_err());
        db.command(&format!("DROP TABLE {DATABASE}.{dict_table} SYNC"))
            .unwrap();
        store::validate_inserted_batch(
            &db,
            &clone,
            &loaded_metadata,
            ids[0],
            ids[cells - 1],
            cells,
        )
        .unwrap();
        db.command(&format!("ALTER TABLE meteogram_native.{clone} UPDATE temperature=[toUInt32(2139095040),0,0,0,0,0] WHERE cell_id=11 SETTINGS mutations_sync=2")).unwrap();
        assert!(store::validate(root.path(), &clone, "meteogram_native", false).is_err());
        assert!(store::validate_inserted_batch(
            &db,
            &clone,
            &loaded_metadata,
            ids[0],
            ids[cells - 1],
            cells
        )
        .is_err());
        assert!(store::validate_inserted_batch(
            &db,
            &clone,
            &loaded_metadata,
            ids[0],
            ids[cells - 1],
            cells + 1
        )
        .is_err());
        assert!(!root.path().join("validated.json").exists());
        db.command(&format!("DROP TABLE meteogram_native.{clone} SYNC"))
            .unwrap();
    }));
    let _ = db.command(&format!(
        "DROP TABLE IF EXISTS meteogram_native.{table} SYNC"
    ));
    let _ = db.command(&format!(
        "DROP TABLE IF EXISTS meteogram_native.{table}_clone SYNC"
    ));
    let _ = db.command(&format!(
        "DROP TABLE IF EXISTS {DATABASE}.{table}_dict SYNC"
    ));
    assert!(result.is_ok());
}

#[test]
#[ignore = "requires the isolated loopback ClickHouse server"]
fn parallel_load_failure_drains_insert_workers_before_returning() {
    let root = tempfile::tempdir().unwrap();
    atomic(
        root.path().join("validated.json"),
        &json!({"table":"stale_prior_attempt"}),
    )
    .unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_failed_load_{}", uuid::Uuid::new_v4().simple());
    let cells = 8200;
    let ids: Vec<_> = (0..cells as u32).collect();
    save_cells(
        &root.path().join("cells.npz"),
        &ids,
        &vec![41.; cells],
        &vec![-74.; cells],
    )
    .unwrap();
    atomic(root.path().join("metadata.json"),&json!({"model":"hrrr","cells":cells,"levels":[1000],"leads":[0],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]})).unwrap();
    let mut p: Vec<u8> = (0..cells).flat_map(|_| 1f32.to_le_bytes()).collect();
    p[..4].copy_from_slice(&f32::NAN.to_le_bytes());
    fs::write(
        root.path().join("lead-000.npy"),
        npy_bytes("<f4", &[cells, 1, 1], &p),
    )
    .unwrap();
    let g: Vec<u8> = (0..cells * 2).flat_map(|_| 1f32.to_le_bytes()).collect();
    fs::write(
        root.path().join("surface-000.npy"),
        npy_bytes("<f4", &[cells, 2], &g),
    )
    .unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        assert!(store::load_with_workers(root.path(), root.path(), &table, Some(4)).is_err());
        assert!(!root.path().join("validated.json").exists());
        assert_eq!(db.scalar(&format!("SELECT count() FROM system.processes WHERE startsWith(query,'INSERT INTO meteogram_native.{table} ')")).unwrap(),0);
    }));
    db.command(&format!(
        "DROP TABLE IF EXISTS meteogram_native.{table} SYNC"
    ))
    .unwrap();
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
fn legacy_null_storage_marker_remains_publishable() {
    let root = tempfile::tempdir().unwrap();
    let (p, e) = candidate_fixture(root.path(), 6);
    let mut v = read_json(p.join("validated.json")).unwrap();
    v["storage"] = Value::Null;
    atomic(p.join("validated.json"), &v).unwrap();
    publish(root.path(), "gfs", e, false).unwrap();
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
        fs::create_dir_all(descriptor(root.path(), &e).unwrap()).unwrap();
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

#[tokio::test]
async fn cancellation_drains_source_before_unlock() {
    let root = tempfile::tempdir().unwrap();
    let lock = Lock::acquire(root.path(), "update", true, true).unwrap();
    let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
    let (done_tx, done_rx) = tokio::sync::oneshot::channel();
    struct Done(Option<tokio::sync::oneshot::Sender<()>>);
    impl Drop for Done {
        fn drop(&mut self) {
            let _ = self.0.take().unwrap().send(());
        }
    }
    let task = tokio::spawn(async move {
        let _done = Done(Some(done_tx));
        let _ = ready_tx.send(());
        std::future::pending::<()>().await;
    });
    let mut sources = crate::pipeline::SourceTasks::default();
    sources.track(&task);
    drop(task);
    ready_rx.await.unwrap();
    sources.drain().await;
    done_rx.await.unwrap();
    assert!(Lock::acquire(root.path(), "update", true, true).is_err());
    drop(lock);
    assert!(Lock::acquire(root.path(), "update", true, true).is_ok());
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
#[ignore = "requires the isolated loopback ClickHouse server"]
fn legacy_validation_covers_sparse_ranges_and_balanced_duplicates() {
    let root = tempfile::tempdir().unwrap();
    let db = Db::new().unwrap();
    let table = format!("rust_legacy_{}", uuid::Uuid::new_v4().simple());
    let ids = [11, 22, 32770, 65540, 65541];
    save_cells(&root.path().join("cells.npz"), &ids, &[41.; 5], &[-74.; 5]).unwrap();
    let m = json!({"model":"gfs","init_time":"2026-10-17T18:00:00","snapshot_id":"test","coverage":"conus","cells":5,"levels":[1000,500],"leads":[0,1,2],"variables":["temperature"],"surface_variables":["temperature_2m","dew_point_temperature_2m"]});
    atomic(root.path().join("metadata.json"), &m).unwrap();
    db.command(&format!("CREATE TABLE meteogram_native.{table} (revision_id UInt32,cell_id UInt32,lead_hour UInt16,temperature Array(Float32),temperature_2m Float32,dew_point_temperature_2m Float32) ENGINE=MergeTree ORDER BY (cell_id,lead_hour)")).unwrap();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let mut values = vec![];
        for id in ids {
            for lead in 0..3 {
                values.push(format!("(1,{id},{lead},[1.0,2.0],3.0,4.0)"));
            }
        }
        db.command(&format!(
            "INSERT INTO meteogram_native.{table} VALUES {}",
            values.join(",")
        ))
        .unwrap();
        store::validate(root.path(), &table, DATABASE, false).unwrap();
        db.command(&format!("ALTER TABLE meteogram_native.{table} DELETE WHERE (cell_id=11 AND lead_hour=0) OR (cell_id=22 AND lead_hour=1) SETTINGS mutations_sync=2")).unwrap();
        db.command(&format!("INSERT INTO meteogram_native.{table} VALUES (1,11,1,[1.0,2.0],3.0,4.0),(1,22,0,[1.0,2.0],3.0,4.0)")).unwrap();
        assert_eq!(
            db.scalar(&format!("SELECT count() FROM meteogram_native.{table}"))
                .unwrap(),
            15
        );
        assert!(store::validate(root.path(), &table, DATABASE, false).is_err());
    }));
    let _ = db.command(&format!(
        "DROP TABLE IF EXISTS meteogram_native.{table} SYNC"
    ));
    assert!(result.is_ok());
}
