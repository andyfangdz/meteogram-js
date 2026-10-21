//! Poll immutable source snapshots, commit each complete hour, and resume work.
use crate::{
    db::Db,
    runs::{self, Book},
    source,
    state::*,
    store,
};
use anyhow::{Context, Result};
use futures::{stream, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
    time::{Duration, Instant},
};

#[derive(Clone, Serialize, Deserialize)]
struct Work {
    model: String,
    pin: Value,
    lead: u16,
    entry: Entry,
    table: String,
}
pub fn find(root: &Path, model: &str, product: &str, init: &str) -> Result<Option<(Entry, Value)>> {
    let c = catalog(root)?;
    let mut found = None;
    for e in runs::direct(&c, false) {
        let m = read_json(descriptor(root, &e)?.join("metadata.json"))?;
        if m["model"] == model
            && m["dataset"] == product
            && m["init_time"] == init
            && found.as_ref().is_none_or(|(_, old): &(Entry, Value)| {
                old["leads"].as_array().unwrap().len() < m["leads"].as_array().unwrap().len()
            })
        {
            found = Some((e, m));
        }
    }
    Ok(found)
}
pub fn shared_cells(root: &Path, source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for name in ["cells.bin.zst"] {
        let src = source.join(name);
        if !src.exists() {
            continue;
        }
        let hash = crate::dictionary::file_hash(&src)?;
        let grids = root.join("grids");
        fs::create_dir_all(&grids)?;
        let canonical = grids.join(format!("{hash}-{name}"));
        if !canonical.exists() {
            match fs::hard_link(&src, &canonical) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e.into()),
            }
        }
        anyhow::ensure!(
            crate::dictionary::file_hash(&canonical)? == hash,
            "Coordinate cache identity differs"
        );
        fs::hard_link(canonical, target.join(name))?;
        fs::File::open(&grids)?.sync_all()?;
        return Ok(());
    }
    anyhow::bail!("Missing coordinates")
}
fn table_exists(db: &Db, table: &str) -> Result<bool> {
    ident(table)?;
    Ok(db.scalar(&format!(
        "SELECT count() FROM system.tables WHERE database='{DATABASE}' AND name='{table}'"
    ))? == 1)
}
pub fn attach_lead(
    root: &Path,
    model: &str,
    path: &Path,
    private: &Entry,
    table: &str,
    fault: Option<&str>,
) -> Result<Entry> {
    ident(table)?;
    let m = read_json(path.join("metadata.json"))?;
    check_segment_metadata(&m)?;
    anyhow::ensure!(
        m["leads"].as_array().map(Vec::len) == Some(1) && m["storage"]["partitioned_lead"] == true,
        "Expected a validated single-hour partition"
    );
    let lead = m["leads"][0].as_u64().context("Lead")? as u16;
    let init = m["init_time"].as_str().context("Init")?;
    let product = m["dataset"].as_str().context("Dataset")?;
    let prior = find(root, model, product, init)?;
    for referenced in runs::direct(&catalog(root)?, true)
        .into_iter()
        .filter(|e| e.table == table)
    {
        let old = read_json(descriptor(root, &referenced)?.join("metadata.json"))?;
        anyhow::ensure!(
            old["model"] == model && old["dataset"] == product && old["init_time"] == init,
            "A table cannot contain another forecast run"
        );
    }
    if let Some((e, old)) = &prior {
        if old["leads"].as_array().unwrap().contains(&json!(lead)) {
            return Ok(e.clone());
        }
        anyhow::ensure!(
            partitioned(old) && e.table == table,
            "Run identity/table changed"
        );
    }
    runs::binding(path, private, &m)?;
    let mut books = prior
        .as_ref()
        .map(|(_, m)| runs::books(m))
        .transpose()?
        .unwrap_or_default();
    let mut count = 0;
    for b in &books {
        crate::dictionary::count_entries(&mut count, b.entries)?;
    }
    let entries = m["storage"]["dictionary"]["entries"]
        .as_u64()
        .context("Dictionary entries")? as usize;
    crate::dictionary::count_entries(&mut count, entries)?;
    let db = Db::new()?;
    db.command(&format!(
        "CREATE TABLE IF NOT EXISTS {DATABASE}.{table} AS {DATABASE}.{}",
        ident(&private.table)?
    ))?;
    let schema = |name: &str| {
        db.json(&format!("SELECT name,type FROM system.columns WHERE database='{DATABASE}' AND table='{name}' ORDER BY position"))
    };
    anyhow::ensure!(
        schema(table)? == schema(&private.table)?,
        "Run physical schema changed"
    );
    let key = |name: &str| {
        db.json(&format!("SELECT engine,partition_key,sorting_key FROM system.tables WHERE database='{DATABASE}' AND name='{name}'"))
    };
    anyhow::ensure!(
        key(table)? == key(&private.table)?,
        "Run storage keys changed"
    );
    let nc = m["cells"].as_u64().context("Cells")? as usize;
    // This lead is absent from every published manifest. Dropping its private
    // retry partitions cannot change any hour visible to an existing reader.
    for spatial in 0..nc.div_ceil(32768) {
        let count=db.scalar(&format!("SELECT count() FROM {DATABASE}.{table} WHERE lead_hour={lead} AND tile_id>={} AND tile_id<{}",spatial*128,(spatial+1)*128))?;
        if count > 0 {
            db.command(&format!(
                "ALTER TABLE {DATABASE}.{table} DROP PARTITION tuple({lead},{spatial})"
            ))?;
        }
        db.command(&format!("ALTER TABLE {DATABASE}.{table} ATTACH PARTITION tuple({lead},{spatial}) FROM {DATABASE}.{}",private.table))?;
    }
    anyhow::ensure!(
        fault != Some("after_attach"),
        "Injected interruption after attach"
    );
    let proof = store::validate(path, table, DATABASE, true)?;
    anyhow::ensure!(
        proof["full_values_checked"] == true && proof["staged_comparison"] == true,
        "Missing attached source proof"
    );
    let (child_path, mut child) = candidate(root, model)?;
    child.table = table.to_owned();
    child.owned = false;
    atomic(child_path.join("entry.json"), &child)?;
    fs::copy(path.join("metadata.json"), child_path.join("metadata.json"))?;
    fs::copy(
        path.join("validated.json"),
        child_path.join("validated.json"),
    )?;
    fs::copy(
        path.join("validated.json"),
        child_path.join("source-validation.json"),
    )?;
    crate::dictionary::copy(path, &child_path, &m)?;
    crate::clouds::copy(path, &child_path, &m)?;
    shared_cells(root, path, &child_path)?;
    for (from, to) in [
        (format!("lead-{lead:03}.json"), "acquisition.json"),
        ("load-timings.json".to_owned(), "load-timings.json"),
    ] {
        if path.join(&from).exists() {
            fs::copy(path.join(from), child_path.join(to))?;
        }
    }
    for file in [
        "metadata.json",
        "validated.json",
        "source-validation.json",
        "values.dict.zst",
    ] {
        fs::File::open(child_path.join(file))?.sync_all()?;
    }
    fs::File::open(&child_path)?.sync_all()?;
    books.push(Book {
        lead,
        revision: child.revision,
        entries,
        metadata_sha256: crate::dictionary::file_hash(&child_path.join("metadata.json"))?,
        snapshot_id: m["snapshot_id"].as_str().context("Snapshot")?.to_owned(),
    });
    books.sort_by_key(|b| b.lead);
    let expected = expected_leads(&m)?;
    let have = books.iter().map(|b| b.lead).collect::<Vec<_>>();
    let (parent_path, mut parent) = candidate(root, model)?;
    parent.table = table.to_owned();
    atomic(parent_path.join("entry.json"), &parent)?;
    let mut manifest = m.clone();
    manifest.as_object_mut().unwrap().remove("staging");
    manifest.as_object_mut().unwrap().remove("native_clouds");
    manifest["leads"] = json!(have);
    manifest["storage"] = json!({"layout":"lead_partition_tiles_v1","version":1,"encoding":runs::ENCODING,"tile_size":256,"axes":"level","packing":"uint16x8_le","element_type":"UInt128","index_type":"UInt16","expected_leads":expected,"complete":have==expected,"codebook_entries":count,"books":books});
    atomic(parent_path.join("metadata.json"), &manifest)?;
    shared_cells(root, path, &parent_path)?;
    let aggregate = runs::validate_manifest(root, &parent, &manifest, false)?;
    atomic(parent_path.join("validated.json"), &aggregate)?;
    anyhow::ensure!(
        fault != Some("before_publish"),
        "Injected interruption before publication"
    );
    runs::publish(root, model, parent.clone(), false)?;
    anyhow::ensure!(
        fault != Some("after_publish"),
        "Injected interruption after publication"
    );
    event(
        root,
        json!({"phase":"lead_published","model":model,"init_time":init,"lead":lead,"complete":have==expected,"available_leads":have.len(),"expected_leads":expected.len(),"revision":parent.revision,"codebook_entries":count}),
    )?;
    Ok(parent)
}
fn work_path(root: &Path, w: &Work) -> Result<PathBuf> {
    descriptor(&root.join("incoming"), &w.entry)
}
fn clean_work(root: &Path, w: &Work, journal: &Path) -> Result<()> {
    let path = work_path(root, w)?;
    if path.exists() {
        store::discard(root, &path)?;
        fs::remove_dir_all(path)?;
    }
    if journal.exists() {
        fs::remove_file(journal)?;
    }
    Ok(())
}
fn recover(root: &Path) -> Result<()> {
    let dir = root.join("incoming/state");
    fs::create_dir_all(&dir)?;
    for file in fs::read_dir(dir)? {
        let file = file?.path();
        if file.extension().is_none_or(|x| x != "json") {
            continue;
        }
        let w: Work = serde_json::from_value(read_json(&file)?)?;
        let known = find(
            root,
            &w.model,
            w.pin["dataset"].as_str().context("Dataset")?,
            w.pin["init_time"].as_str().context("Init")?,
        )?;
        let committed =
            known.is_some_and(|(_, m)| m["leads"].as_array().unwrap().contains(&json!(w.lead)));
        let age = now() - timestamp(w.pin["init_time"].as_str().unwrap())?;
        if committed || age > 48. * 3600. {
            clean_work(root, &w, &file)?;
        }
    }
    Ok(())
}
fn key(model: &str, product: &str, init: &str) -> Result<String> {
    Ok(format!(
        "{model}_{}_{}",
        if product == HRRR_SHORT {
            "short"
        } else {
            "main"
        },
        timestamp(init)? as i64
    ))
}
fn new_work(root: &Path, model: &str, pin: &Value, lead: u16) -> Result<(Work, PathBuf)> {
    let id = key(
        model,
        pin["dataset"].as_str().context("Dataset")?,
        pin["init_time"].as_str().context("Init")?,
    )?;
    let journal = root
        .join("incoming/state")
        .join(format!("{id}_{lead:03}.json"));
    if journal.exists() {
        return Ok((serde_json::from_value(read_json(&journal)?)?, journal));
    }
    let table = if let Some((e, _)) = find(
        root,
        model,
        pin["dataset"].as_str().unwrap(),
        pin["init_time"].as_str().unwrap(),
    )? {
        e.table
    } else {
        let run = root.join("incoming/runs").join(format!("{id}.json"));
        if run.exists() {
            read_json(&run)?["table"]
                .as_str()
                .context("Run table")?
                .to_owned()
        } else {
            let table = format!("{model}_run_{}", uuid::Uuid::new_v4().simple());
            atomic(
                run,
                &json!({"model":model,"dataset":pin["dataset"],"init_time":pin["init_time"],"table":table,"owned":true}),
            )?;
            table
        }
    };
    let (_, entry) = candidate(&root.join("incoming"), model)?;
    let w = Work {
        model: model.to_owned(),
        pin: pin.clone(),
        lead,
        entry,
        table,
    };
    atomic(&journal, &w)?;
    Ok((w, journal))
}
async fn ingest(
    root: &Path,
    model: &str,
    pin: &Value,
    lead: u16,
    source: Arc<source::Source>,
) -> Result<()> {
    let (w, journal) = new_work(root, model, pin, lead)?;
    let path = work_path(root, &w)?;
    let metadata = path.join("metadata.json");
    let ready = metadata.exists()
        && read_json(&metadata)?["staging"]["complete"] == true
        && path.join(format!("lead-{lead:03}.json")).exists();
    if !ready {
        source::fetch_hour(model, &path, w.pin.clone(), lead, &source).await?;
    }
    if model == "hrrr" {
        source::fetch_clouds(&path, Some(&source)).await?;
    }
    let r = root.to_owned();
    tokio::task::spawn_blocking(move || -> Result<()> {
        let db = Db::new()?;
        let valid = table_exists(&db, &w.entry.table)?
            && store::validate(&path, &w.entry.table, DATABASE, true).is_ok();
        if !valid {
            db.command(&format!(
                "DROP TABLE IF EXISTS {DATABASE}.{} SYNC",
                ident(&w.entry.table)?
            ))?;
            store::load_lead(&r, &path, &w.entry.table, w.lead)?;
        }
        attach_lead(&r, &w.model, &path, &w.entry, &w.table, None)?;
        clean_work(&r, &w, &journal)
    })
    .await??;
    Ok(())
}
// Progress acts as a durable completion preference across polls and restarts.
// A newer initialization must not displace a runnable, more advanced forecast.
type CompletionTargets = BTreeMap<String, (usize, String)>;
fn prefer_completion(
    targets: &mut CompletionTargets,
    product: &str,
    init: &str,
    have: usize,
) -> Result<()> {
    let replace = match targets.get(product) {
        None => true,
        Some((old_have, old_init)) => {
            have > *old_have || (have == *old_have && timestamp(init)? > timestamp(old_init)?)
        }
    };
    if replace {
        targets.insert(product.to_owned(), (have, init.to_owned()));
    }
    Ok(())
}
fn completion_rank(pin: &Value, targets: &CompletionTargets) -> u8 {
    let product = pin["dataset"].as_str().unwrap();
    if targets
        .get(product)
        .is_some_and(|(_, init)| pin["init_time"] == *init)
    {
        // Complete the HRRR long-range base before spending its budget on short runs.
        if product == "noaa-hrrr-forecast-48-hour-virtual" {
            0
        } else {
            1
        }
    } else {
        2
    }
}

pub async fn poll(root: PathBuf, models: Vec<String>, budget_seconds: u64) -> Result<i32> {
    poll_with_stop(
        root,
        models,
        budget_seconds,
        Arc::new(AtomicBool::new(false)),
    )
    .await
}
async fn poll_with_stop(
    root: PathBuf,
    models: Vec<String>,
    budget_seconds: u64,
    stop: Arc<AtomicBool>,
) -> Result<i32> {
    let _lock = Lock::acquire(&root, "update", true, true)?;
    let maintenance_started = Instant::now();
    let began = now();
    let r = root.clone();
    let examined = tokio::task::spawn_blocking(move || -> Result<usize> {
        recover(&r)?;
        store::collect(&r, 600.)?;
        store::sweep_unreferenced(&r)
    })
    .await??;
    let maintenance_seconds = maintenance_started.elapsed().as_secs_f64();
    event(
        &root,
        json!({"phase":"maintenance_completed","seconds":maintenance_seconds,"revisions_examined":examined}),
    )?;
    let discovery_started = Instant::now();
    let c = catalog(&root)?;
    let mut products = Vec::new();
    for model in &models {
        products.push((model.clone(), dataset(model)?.to_owned()));
        if model == "hrrr" {
            products.push((model.clone(), HRRR_SHORT.to_owned()));
        }
    }
    let checks = products
        .into_iter()
        .map(|(model, product)| {
            let tracked = c
                .building
                .get(&model)
                .into_iter()
                .flatten()
                .filter_map(|e| read_json(descriptor(&root, e).ok()?.join("metadata.json")).ok())
                .filter(|m| m["dataset"] == product)
                .filter_map(|m| m["init_time"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let completed = c
                .complete_runs
                .get(&model)
                .into_iter()
                .flatten()
                .chain(c.hrrr_long.iter())
                .filter_map(|e| read_json(descriptor(&root, e).ok()?.join("metadata.json")).ok())
                .filter(|m| m["model"] == model && m["dataset"] == product)
                .filter_map(|m| m["init_time"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let mut floor = c
                .complete_runs
                .get(&model)
                .and_then(|v| v.get(1))
                .map(|e| read_json(descriptor(&root, e)?.join("metadata.json")))
                .transpose()?
                .map(|m| timestamp(m["init_time"].as_str().unwrap()))
                .transpose()?;
            if model == "hrrr" && product == dataset("hrrr")? {
                floor = c
                    .hrrr_long
                    .as_ref()
                    .map(|e| read_json(descriptor(&root, e)?.join("metadata.json")))
                    .transpose()?
                    .map(|m| timestamp(m["init_time"].as_str().unwrap()))
                    .transpose()?;
            }
            Ok::<_, anyhow::Error>(async move {
                let result =
                    source::availability_filtered(&model, &product, &tracked, &completed, floor)
                        .await;
                (model, product, result)
            })
        })
        .collect::<Vec<_>>();
    let mut discovery = Vec::new();
    for check in checks {
        discovery.push(check?);
    }
    let checks = stream::iter(discovery)
        .buffer_unordered(2)
        .collect::<Vec<_>>()
        .await;
    let mut queues: BTreeMap<String, Vec<(Value, u16, Arc<source::Source>)>> = BTreeMap::new();
    let mut failures = Vec::new();
    let prior_availability = if root.join("availability.json").exists() {
        read_json(root.join("availability.json"))?
    } else {
        json!({"sources":[]})
    };
    let mut availability = Vec::new();
    let mut waiting = 0usize;
    let mut completion = BTreeMap::new();
    for (model, product, result) in checks {
        match result {
            Err(e)=>failures.push(json!({"model":model,"dataset":product,"phase":"availability","error":format!("{e:#}")})),
            Ok(check)=>for run in check.runs {
                let init=run.pin["init_time"].as_str().context("Init")?;
                let known=find(&root,&model,&product,init)?;
                let have=known.as_ref().map(|(_,m)|m["leads"].as_array().unwrap().clone()).unwrap_or_default();
                let floor=c.complete_runs.get(&model).filter(|v|v.len()>=2).map(|v|read_json(descriptor(&root,&v[1])?.join("metadata.json"))).transpose()?.map(|m|timestamp(m["init_time"].as_str().unwrap())).transpose()?;
                let t=timestamp(init)?;
                let prior = prior_availability["sources"].as_array().and_then(|sources| sources.iter().find(|s| s["dataset"] == product && s["init_time"] == init));
                let mut ready_at = serde_json::Map::new();
                for lead in &run.ready {
                    if run.cloud_ready.as_ref().is_none_or(|cloud| cloud.contains(lead)) {
                        let key = lead.to_string();
                        let first = prior.and_then(|p| p["ready_at"][&key].as_f64()).unwrap_or_else(now);
                        ready_at.insert(key, json!(first));
                    } else if !have.contains(&json!(lead)) {
                        waiting += 1;
                        let already_waiting = prior.is_some_and(|p| p["ready_leads"].as_array().is_some_and(|v| v.contains(&json!(lead))) && p["cloud_ready"].as_array().is_some_and(|v| !v.contains(&json!(lead))));
                        if !already_waiting {
                            event(&root, json!({"phase":"lead_waiting","detail":{"model":model,"init_time":init,"lead":lead,"reason":"Cloud data not yet available"}}))?;
                        }
                    }
                }
                availability.push(json!({"model":model,"dataset":product,"init_time":init,"snapshot_id":check.source.snapshot,"ready_leads":run.ready,"cloud_ready":run.cloud_ready,"ready_at":ready_at,"expected_leads":run.expected,"missing_references":run.missing,"payload_reads":0}));
                if known.as_ref().is_some_and(|(_,m)|complete(m).unwrap_or(false)) || (known.is_none() && floor.is_some_and(|old|t<=old)) {continue;}
                let mut full = run.pin.clone();
                full["leads"] = json!(run.expected);
                if run.ready == run.expected && freshness(&full)?["available"] == true {
                    prefer_completion(&mut completion, &product, init, have.len())?;
                }
                for lead in run.ready {
                    if !have.contains(&json!(lead)) && run.cloud_ready.as_ref().is_none_or(|cloud| cloud.contains(&lead)) {
                        let mut pin = run.pin.clone();
                        pin["source_ready_at"] = ready_at[&lead.to_string()].clone();
                        queues.entry(model.clone()).or_default().push((pin,lead,check.source.clone()));
                    }
                }
            }
        }
    }
    atomic(
        root.join("availability.json"),
        &json!({"time":now(),"sources":availability,"failures":failures}),
    )?;
    for jobs in queues.values_mut() {
        jobs.sort_by(|(a, ha, _), (b, hb, _)| {
            completion_rank(a, &completion)
                .cmp(&completion_rank(b, &completion))
                .then_with(|| {
                    timestamp(b["init_time"].as_str().unwrap())
                        .unwrap()
                        .total_cmp(&timestamp(a["init_time"].as_str().unwrap()).unwrap())
                })
                .then(ha.cmp(hb))
        });
    }
    let discovery_seconds = discovery_started.elapsed().as_secs_f64();
    // Discovery/cleanup must not consume the time reserved for starting ingestion.
    let started = Instant::now();
    event(
        &root,
        json!({"phase":"ingestion_planned","discovery_seconds":discovery_seconds,"completion_targets":completion,"queued_leads":queues.values().map(Vec::len).sum::<usize>()}),
    )?;
    // Two regional extractors plus one global-model extractor keep small
    // GFS/AIFS hour updates moving while the large native grids download.
    let regional_gate = Arc::new(tokio::sync::Semaphore::new(2));
    let global_gate = Arc::new(tokio::sync::Semaphore::new(1));
    let results=stream::iter(queues).map(|(model,jobs)| {
        let root=root.clone();let gate=if grid_relative(&model){regional_gate.clone()}else{global_gate.clone()};let stop=stop.clone();
        async move {
            let mut completed=0;let mut errors=Vec::new();let mut deferred=0;
            for (pin,lead,src) in jobs {
                if stop.load(Ordering::Relaxed) || started.elapsed()>=Duration::from_secs(budget_seconds) {break;}
                let permit=gate.clone().acquire_owned().await.unwrap();
                if stop.load(Ordering::Relaxed) || started.elapsed()>=Duration::from_secs(budget_seconds) {drop(permit);break;}
                match ingest(&root,&model,&pin,lead,src).await {
                    Ok(())=>{
                        completed+=1;
                        event(&root,json!({"phase":"lead_ingested","model":model,"dataset":pin["dataset"],"init_time":pin["init_time"],"lead":lead,"lag_seconds":pin["source_ready_at"].as_f64().map(|t|(now()-t).max(0.))}))?;
                    },
                    Err(e) if e.downcast_ref::<source::SourcePending>().is_some()=>{
                        deferred+=1;
                        event(&root,json!({"phase":"lead_waiting","detail":{"model":model,"init_time":pin["init_time"],"lead":lead,"reason":e.to_string()}}))?;
                    },
                    Err(e)=>{let error=json!({"model":model,"init_time":pin["init_time"],"lead":lead,"error":format!("{e:#}")});event(&root,json!({"phase":"lead_failed","detail":error}))?;errors.push(error);}
                }
                drop(permit);
            }
            Ok::<_,anyhow::Error>((completed,errors,deferred))
        }
    }).buffer_unordered(4).collect::<Vec<_>>().await;
    let mut completed = 0;
    for result in results {
        let (count, errors, deferred) = result?;
        waiting += deferred;
        completed += count;
        failures.extend(errors);
    }
    atomic(
        root.join("last-update.json"),
        &json!({"started":began,"time":now(),"completed_leads":completed,"waiting_leads":waiting,"failures":failures,"maintenance_seconds":maintenance_seconds,"discovery_seconds":discovery_seconds,"ingestion_seconds":started.elapsed().as_secs_f64(),"incremental":true,"implementation":"rust"}),
    )?;
    Ok(if failures.is_empty() { 0 } else { 1 })
}
pub async fn watch(root: PathBuf, poll_seconds: f64) -> Result<()> {
    anyhow::ensure!(
        poll_seconds.is_finite() && poll_seconds >= 1.,
        "Poll interval must be at least one second"
    );
    let _lock = Lock::acquire(&root, "watch", true, true)?;
    let stop = Arc::new(AtomicBool::new(false));
    let requested = stop.clone();
    let signal = tokio::spawn(async move {
        let mut term =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).unwrap();
        let mut interrupt =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt()).unwrap();
        tokio::select! {_=term.recv()=>{},_=interrupt.recv()=>{}}
        requested.store(true, Ordering::Relaxed);
    });
    while !stop.load(Ordering::Relaxed) {
        let began = now();
        let result = poll_with_stop(
            root.clone(),
            MODELS.iter().map(|m| (*m).to_owned()).collect(),
            (poll_seconds * 0.8).max(1.) as u64,
            stop.clone(),
        )
        .await;
        let code = match result {
            Ok(code) => code,
            Err(e) => {
                event(
                    &root,
                    json!({"phase":"poll_failed","error":format!("{e:#}")}),
                )?;
                1
            }
        };
        let next = (began + poll_seconds).max(now() + 1.);
        atomic(
            root.join("schedule.json"),
            &json!({"next_update":next,"last_exit":code,"poll_seconds":poll_seconds,"incremental":true}),
        )?;
        while !stop.load(Ordering::Relaxed) && now() < next {
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    signal.abort();
    Ok(())
}

#[cfg(test)]
mod scheduling_tests {
    use super::*;

    #[test]
    fn newer_runs_do_not_starve_a_completion_in_progress() {
        let product = "noaa-gfs-forecast-virtual";
        let older = "2026-10-20T06:00:00";
        let newer = "2026-10-20T12:00:00";
        for have in 82..141 {
            // Rebuild from durable progress each poll, including after restart.
            let mut targets = CompletionTargets::new();
            prefer_completion(&mut targets, product, newer, 0).unwrap();
            prefer_completion(&mut targets, product, older, have).unwrap();
            assert_eq!(targets[product].1, older);
            assert!(
                completion_rank(&json!({"dataset":product,"init_time":older}), &targets)
                    < completion_rank(&json!({"dataset":product,"init_time":newer}), &targets)
            );
        }
        let mut targets = CompletionTargets::new();
        prefer_completion(&mut targets, product, older, 0).unwrap();
        prefer_completion(&mut targets, product, newer, 0).unwrap();
        assert_eq!(targets[product].1, newer);
    }

    #[test]
    fn hrrr_long_base_gets_capacity_before_hourly_churn() {
        let long = dataset("hrrr").unwrap();
        let mut targets = CompletionTargets::new();
        prefer_completion(&mut targets, long, "2026-10-21T00:00:00", 5).unwrap();
        prefer_completion(&mut targets, HRRR_SHORT, "2026-10-21T01:00:00", 18).unwrap();
        let long = json!({"dataset":long,"init_time":"2026-10-21T00:00:00"});
        let short = json!({"dataset":HRRR_SHORT,"init_time":"2026-10-21T01:00:00"});
        let newest = json!({"dataset":HRRR_SHORT,"init_time":"2026-10-21T02:00:00"});
        assert!(completion_rank(&long, &targets) < completion_rank(&short, &targets));
        assert!(completion_rank(&short, &targets) < completion_rank(&newest, &targets));
    }
}
