//! Immutable published manifests over append-only, individually validated leads.
use crate::{db::Db, dictionary::Dictionary, state::*};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    path::{Path, PathBuf},
    sync::{Arc, Mutex, Weak},
};

pub const ENCODING: &str = "float32_dictionary_per_lead_v1";
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Book {
    pub lead: u16,
    pub revision: String,
    pub entries: usize,
    pub metadata_sha256: String,
    pub snapshot_id: String,
}
pub type BookCache = Arc<Mutex<HashMap<String, Weak<Dictionary>>>>;
pub fn books(m: &Value) -> Result<Vec<Book>> {
    serde_json::from_value(m["storage"]["books"].clone()).context("Missing per-lead books")
}
pub fn book_entry(root: &Path, b: &Book) -> Result<(Entry, PathBuf, Value)> {
    ident(&b.revision)?;
    let p = root.join("revisions").join(&b.revision);
    anyhow::ensure!(
        crate::dictionary::file_hash(&p.join("metadata.json"))? == b.metadata_sha256,
        "Lead metadata identity changed"
    );
    let e: Entry = serde_json::from_value(read_json(p.join("entry.json"))?)?;
    anyhow::ensure!(
        e.revision == b.revision && !e.owned,
        "Lead descriptors must not own the shared run table"
    );
    let m = read_json(p.join("metadata.json"))?;
    anyhow::ensure!(
        m["leads"] == json!([b.lead]) && m["snapshot_id"] == b.snapshot_id,
        "Wrong lead descriptor"
    );
    Ok((e, p, m))
}
pub fn binding(path: &Path, e: &Entry, m: &Value) -> Result<Value> {
    let v = read_json(path.join("validated.json"))?;
    anyhow::ensure!(
        v["table"] == e.table
            && v["database"] == e.database
            && v["init_time"] == m["init_time"]
            && v["snapshot_id"] == m["snapshot_id"]
            && v.get("storage").unwrap_or(&Value::Null) == m.get("storage").unwrap_or(&Value::Null),
        "Candidate lacks matching validation"
    );
    Ok(v)
}
pub fn root_for(path: &Path) -> Result<&Path> {
    path.parent()
        .and_then(Path::parent)
        .context("Descriptor must be inside revisions")
}
pub fn validate_manifest(root: &Path, e: &Entry, m: &Value, full: bool) -> Result<Value> {
    check_metadata(m)?;
    crate::dictionary::widths(m)?;
    anyhow::ensure!(
        partitioned(m) && m["storage"]["encoding"] == ENCODING && m["storage"]["tile_size"] == 256,
        "Unsupported lead partition format"
    );
    let mut actual = Vec::new();
    let mut count = 0;
    for b in books(m)? {
        let (child, p, cm) = book_entry(root, &b)?;
        check_segment_metadata(&cm)?;
        anyhow::ensure!(
            child.table == e.table
                && child.database == e.database
                && cm["storage"]["lead_hour"] == b.lead
                && cm["storage"]["partitioned_lead"] == true,
            "Lead table/partition changed"
        );
        for key in [
            "model",
            "dataset",
            "init_time",
            "cells",
            "levels",
            "variables",
            "surface_variables",
            "grid",
            "units",
        ] {
            anyhow::ensure!(
                cm[key] == m[key],
                "Lead source schema or initialization differs: {key}"
            );
        }
        crate::clouds::binding(&p, &cm)?;
        let proof = if full {
            crate::store::validate(&p, &e.table, &e.database, false)?
        } else {
            binding(&p, &child, &cm)?
        };
        anyhow::ensure!(
            proof["full_values_checked"] == true
                && cm["storage"]["dictionary"]["entries"].as_u64() == Some(b.entries as u64),
            "Incomplete lead value proof"
        );
        let source = read_json(p.join("source-validation.json"))?;
        anyhow::ensure!(
            source["full_values_checked"] == true
                && source["staged_comparison"] == true
                && source["table"] == e.table
                && source["database"] == e.database
                && source["init_time"] == cm["init_time"]
                && source["snapshot_id"] == cm["snapshot_id"]
                && source["storage"] == cm["storage"],
            "Missing immutable staged source proof"
        );
        crate::dictionary::count_entries(&mut count, b.entries)?;
        actual.push(b.lead);
    }
    anyhow::ensure!(
        !actual.is_empty()
            && actual.windows(2).all(|w| w[0] < w[1])
            && json!(actual) == m["leads"]
            && m["storage"]["codebook_entries"].as_u64() == Some(count as u64),
        "Missing, duplicate or substituted forecast hours"
    );
    let nc = m["cells"].as_u64().context("Cells")?;
    let values = nc
        * actual.len() as u64
        * (m["levels"].as_array().context("Levels")?.len() as u64
            * m["variables"].as_array().context("Fields")?.len() as u64
            + 2);
    Ok(
        json!({"database":e.database,"table":e.table,"init_time":m["init_time"],"snapshot_id":m["snapshot_id"],"storage":m["storage"],"rows":nc*actual.len() as u64,"storage_rows":nc.div_ceil(256)*actual.len() as u64,"full_values_checked":true,"staged_comparison":true,"source_proof":"immutable_per_lead_staged_comparisons","palette_values_checked":count,"logical_weather_values":values,"finite_validation_mode":"individually_validated_lead_partitions","complete":complete(m)?,"validated_at":now()}),
    )
}
pub fn load_books(
    root: &Path,
    e: &Entry,
    m: &Value,
    cache: Option<&BookCache>,
) -> Result<BTreeMap<u16, Arc<Dictionary>>> {
    validate_manifest(root, e, m, false)?;
    crate::dictionary::widths(m)?;
    let mut result = BTreeMap::new();
    let mut entries = 0;
    for b in books(m)? {
        let before = entries;
        crate::dictionary::count_entries(&mut entries, b.entries)?;
        let cached = cache.and_then(|c| c.lock().unwrap().get(&b.revision).and_then(Weak::upgrade));
        let dictionary = if let Some(d) = cached {
            d
        } else {
            let (child, p, cm) = book_entry(root, &b)?;
            anyhow::ensure!(
                child.table == e.table && child.database == e.database,
                "Lead table differs"
            );
            binding(&p, &child, &cm)?;
            let d = Arc::new(Dictionary::load_values_budget(
                &p,
                &cm,
                crate::dictionary::MAX_DICTIONARY_ENTRIES - before,
            )?);
            if let Some(cache) = cache {
                let mut cache = cache.lock().unwrap();
                cache.retain(|_, w| w.strong_count() > 0);
                cache.insert(b.revision.clone(), Arc::downgrade(&d));
            }
            d
        };
        anyhow::ensure!(
            dictionary.entry_count()? == b.entries,
            "Lead dictionary count differs"
        );
        anyhow::ensure!(
            result.insert(b.lead, dictionary).is_none(),
            "Duplicate hour dictionary"
        );
    }
    anyhow::ensure!(
        entries
            == m["storage"]["codebook_entries"]
                .as_u64()
                .context("Missing run book budget")? as usize,
        "Run codebook budget differs"
    );
    Ok(result)
}
pub struct Reader {
    books: BTreeMap<u16, Arc<Dictionary>>,
    ids: Vec<u32>,
}
impl Reader {
    pub fn load(root: &Path, e: &Entry, m: &Value) -> Result<Self> {
        anyhow::ensure!(partitioned(m), "Unsupported forecast run layout");
        let arrays = crate::stage::load_cells(&descriptor(root, e)?)?;
        let ids = arrays["cell_id"]
            .bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|b| u32::from_le_bytes(*b))
            .collect();
        Ok(Self {
            books: load_books(root, e, m, None)?,
            ids,
        })
    }
    pub fn profile(&self, db: &Db, e: &Entry, m: &Value, cell: u32) -> Result<(Vec<Value>, f64)> {
        let point = self
            .ids
            .binary_search(&cell)
            .map_err(|_| anyhow::anyhow!("Unknown cell"))?;
        db.profile_leads(&e.database, &e.table, m, cell, point, &self.books)
    }
}
pub fn direct(c: &Catalog, retired: bool) -> Vec<Entry> {
    let mut entries = c
        .current
        .values()
        .chain(c.previous.values())
        .cloned()
        .collect::<Vec<_>>();
    entries.extend(c.complete_runs.values().flatten().cloned());
    entries.extend(c.building.values().flatten().cloned());
    entries.extend(c.hrrr_long.iter().cloned());
    if retired {
        entries.extend(c.retired.clone());
    }
    entries.sort_by(|a, b| a.revision.cmp(&b.revision));
    entries.dedup_by(|a, b| a.revision == b.revision);
    entries
}
pub fn references(root: &Path, c: &Catalog, retired: bool) -> Result<Vec<Entry>> {
    let mut all = direct(c, retired);
    for e in all.clone() {
        let path = descriptor(root, &e)?;
        let m = read_json(path.join("metadata.json"))?;
        anyhow::ensure!(partitioned(&m), "Unsupported retained run layout");
        for b in books(&m)? {
            all.push(book_entry(root, &b)?.0);
        }
    }
    all.sort_by(|a, b| a.revision.cmp(&b.revision));
    all.dedup_by(|a, b| a.revision == b.revision);
    Ok(all)
}
fn time(root: &Path, e: &Entry) -> Result<f64> {
    timestamp(
        read_json(descriptor(root, e)?.join("metadata.json"))?["init_time"]
            .as_str()
            .context("Missing init")?,
    )
}
fn long(m: &Value) -> bool {
    m["model"] == "hrrr"
        && m["dataset"] == "noaa-hrrr-forecast-48-hour-virtual"
        && m["leads"].as_array().is_some_and(|l| l.len() == 49)
}
pub(crate) fn normalize(root: &Path, c: &mut Catalog, extra: Option<(&str, &Entry)>) -> Result<()> {
    let mut pool = direct(c, true);
    if let Some((_, e)) = extra {
        pool.push(e.clone());
    }
    let mut complete_by_model: BTreeMap<String, Vec<(f64, Entry)>> = BTreeMap::new();
    let mut newest_long: Option<(f64, Entry)> = None;
    for e in pool {
        let p = descriptor(root, &e)?;
        if !p.join("metadata.json").exists() {
            continue;
        }
        let m = read_json(p.join("metadata.json"))?;
        if !complete(&m)? {
            continue;
        }
        check_metadata(&m)?;
        binding(&p, &e, &m)?;
        let t = timestamp(m["init_time"].as_str().context("Missing init")?)?;
        if long(&m)
            && newest_long
                .as_ref()
                .is_none_or(|(old, entry)| t > *old || (t == *old && e.revision > entry.revision))
        {
            newest_long = Some((t, e.clone()));
        }
        complete_by_model
            .entry(m["model"].as_str().context("Model")?.to_owned())
            .or_default()
            .push((t, e));
    }
    c.complete_runs.clear();
    for (name, mut entries) in complete_by_model {
        entries
            .sort_by(|(ta, a), (tb, b)| tb.total_cmp(ta).then_with(|| b.revision.cmp(&a.revision)));
        entries.dedup_by(|(a, _), (b, _)| a == b);
        c.complete_runs
            .insert(name, entries.into_iter().take(2).map(|(_, e)| e).collect());
    }
    c.hrrr_long = newest_long.map(|(_, e)| e);
    Ok(())
}
fn retire_removed(c: &mut Catalog, before: Vec<Entry>) {
    let keep = direct(c, false)
        .into_iter()
        .map(|e| e.revision)
        .collect::<BTreeSet<_>>();
    c.retired.retain(|e| !keep.contains(&e.revision));
    for mut e in before {
        if !keep.contains(&e.revision) && !c.retired.iter().any(|r| r.revision == e.revision) {
            e.retired_at = Some(now());
            c.retired.push(e);
        }
    }
}
fn prune_building(root: &Path, c: &mut Catalog) -> Result<()> {
    let long_time = c.hrrr_long.as_ref().map(|e| time(root, e)).transpose()?;
    for (model, pending) in &mut c.building {
        let floor = c
            .complete_runs
            .get(model)
            .and_then(|v| v.get(1))
            .map(|e| time(root, e))
            .transpose()?;
        let mut keep = Vec::new();
        for entry in pending.drain(..) {
            let m = read_json(descriptor(root, &entry)?.join("metadata.json"))?;
            let t = timestamp(m["init_time"].as_str().context("Init")?)?;
            let needed_long = m["model"] == "hrrr"
                && m["dataset"] == "noaa-hrrr-forecast-48-hour-virtual"
                && long_time.is_none_or(|old| t > old);
            if now() - t <= 48. * 3600. && (floor.is_none_or(|old| t > old) || needed_long) {
                keep.push(entry);
            }
        }
        *pending = keep;
    }
    Ok(())
}
pub fn publish(root: &Path, model: &str, e: Entry, rollback: bool) -> Result<Catalog> {
    publish_with(root, model, e, rollback, None)
}
pub fn publish_cloud_upgrade(root: &Path, prior: &Entry, next: Entry) -> Result<Catalog> {
    crate::clouds::check_upgrade(root, prior, &next)?;
    publish_with(root, "hrrr", next, false, Some(prior))
}
fn publish_with(
    root: &Path,
    model: &str,
    e: Entry,
    rollback: bool,
    cloud_prior: Option<&Entry>,
) -> Result<Catalog> {
    let p = descriptor(root, &e)?;
    let m = read_json(p.join("metadata.json"))?;
    check_metadata(&m)?;
    anyhow::ensure!(m["model"] == model, "Wrong model");
    binding(&p, &e, &m)?;
    validate_manifest(root, &e, &m, false)?;
    let _l = Lock::acquire(root, "catalog", true, false)?;
    let mut c = catalog(root)?;
    let mut before = direct(&c, false);
    before.push(e.clone());
    let t = timestamp(m["init_time"].as_str().context("Missing init")?)?;
    let old = c.current.get(model).cloned();
    let old_time = old.as_ref().map(|e| time(root, e)).transpose()?;
    if !rollback {
        if let Some(old) = &old {
            let om = read_json(descriptor(root, old)?.join("metadata.json"))?;
            if old_time == Some(t) {
                let prior = books(&om)?;
                let next = books(&m)?;
                anyhow::ensure!(
                    e.table == old.table
                        && if let Some(expected) = cloud_prior {
                            expected == old
                        } else {
                            next.len() > prior.len() && prior.iter().all(|b| next.contains(b))
                        },
                    "Incremental publication must preserve and advance validated hours"
                );
            }
        }
    }
    let select = rollback || old_time.is_none_or(|old| t >= old);
    if select {
        anyhow::ensure!(freshness(&m)?["available"] == true, "Selected run is stale");
    }
    normalize(root, &mut c, Some((model, &e)))?;
    let pending = c.building.entry(model.to_owned()).or_default();
    pending.retain(|other| time(root, other).is_ok_and(|ot| ot != t));
    if !complete(&m)? {
        pending.push(e.clone());
    }
    let mut ranked = pending
        .drain(..)
        .map(|v| Ok((time(root, &v)?, v)))
        .collect::<Result<Vec<_>>>()?;
    ranked.sort_by(|(a, _), (b, _)| b.total_cmp(a));
    let long_pending = ranked
        .iter()
        .find(|(_, v)| {
            read_json(descriptor(root, v).unwrap().join("metadata.json"))
                .is_ok_and(|m| m["dataset"] == "noaa-hrrr-forecast-48-hour-virtual")
        })
        .map(|(_, v)| v.clone());
    *pending = ranked.into_iter().take(3).map(|(_, v)| v).collect();
    if let Some(l) = long_pending {
        if !pending.contains(&l) {
            pending.push(l);
        }
    }
    if select {
        c.current.insert(model.to_owned(), e.clone());
    }
    prune_building(root, &mut c)?;
    let current_time = c.current.get(model).map(|v| time(root, v)).transpose()?;
    let previous = if rollback {
        old
    } else {
        c.complete_runs
            .get(model)
            .and_then(|entries| entries.iter().find(|v| time(root, v).ok() != current_time))
            .cloned()
    };
    if let Some(previous) = previous {
        c.previous.insert(model.to_owned(), previous);
    } else {
        c.previous.remove(model);
    }
    retire_removed(&mut c, before);
    c.generation += 1;
    c.published_at = Some(now());
    atomic(root.join("catalog.json"), &c)?;
    Ok(c)
}
pub fn summary(root: &Path, e: &Entry) -> Result<Value> {
    let m = read_json(descriptor(root, e)?.join("metadata.json"))?;
    let expected = expected_leads(&m)?;
    let have: Vec<u16> = serde_json::from_value(m["leads"].clone())?;
    let missing = expected
        .iter()
        .filter(|h| !have.contains(h))
        .copied()
        .collect::<Vec<_>>();
    Ok(
        json!({"revision":e.revision,"init_time":m["init_time"],"dataset":m["dataset"],"available_leads":have,"expected_leads":expected,"missing_leads":missing,"complete":complete(&m)?,"freshness":freshness(&m)?}),
    )
}

pub fn rollback(root: &Path, model: &str) -> Result<()> {
    let _l = Lock::acquire(root, "update", true, true)?;
    let c = catalog(root)?;
    let e = c
        .previous
        .get(model)
        .context("No previous revision")?
        .clone();
    publish(root, model, e, true)?;
    Ok(())
}
pub fn status(root: &Path) -> Result<Value> {
    let _l = Lock::acquire(root, "catalog", false, false)?;
    let c = catalog(root)?;
    let db = Db::new()?;
    let mut models = serde_json::Map::new();
    for (name, e) in &c.current {
        let m = read_json(descriptor(root, e)?.join("metadata.json"))?;
        let scope = format!(
            " WHERE lead_hour IN ({})",
            m["leads"]
                .as_array()
                .context("Leads")?
                .iter()
                .map(Value::to_string)
                .collect::<Vec<_>>()
                .join(",")
        );
        models.insert(name.clone(),json!({"revision":e.revision,"table":e.table,"storage":m.get("storage"),"freshness":freshness(&m)?,"progress":crate::runs::summary(root,e)?,"storage_rows":db.scalar(&format!("SELECT count() FROM {}.{}{scope}",ident(&e.database)?,ident(&e.table)?))?}));
    }
    Ok(json!({"catalog":c,"models":models}))
}
