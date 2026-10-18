use crate::{db::Db, source, state::*, store};
use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::{
    fs,
    path::{Path, PathBuf},
    time::Duration,
};
#[derive(Default)]
pub(crate) struct SourceTasks(Vec<tokio::task::AbortHandle>);
impl SourceTasks {
    pub(crate) fn track<T>(&mut self, task: &tokio::task::JoinHandle<T>) {
        self.0.push(task.abort_handle());
    }
    pub(crate) async fn drain(&self) {
        for task in &self.0 {
            task.abort();
        }
        while self.0.iter().any(|task| !task.is_finished()) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }
}
impl Drop for SourceTasks {
    fn drop(&mut self) {
        for task in &self.0 {
            task.abort();
        }
    }
}
async fn prepare(
    root: PathBuf,
    model: String,
    offsets: Vec<usize>,
    pin: Option<Value>,
) -> Result<Option<(PathBuf, Entry, Value)>> {
    let mut errors = vec![];
    for offset in offsets {
        let (path, entry) = candidate(&root, &model)?;
        let run = async {
            event(
                &root,
                json!({"phase":"select","model":model,"revision":entry.revision,"offset":offset}),
            )?;
            let meta = source::fetch(&model, &path, offset, pin.clone(), true, None).await?;
            check_metadata(&meta)?;
            if let Some(old) = catalog(&root)?.current.get(&model) {
                let om = read_json(descriptor(&root, old)?.join("metadata.json"))?;
                if timestamp(meta["init_time"].as_str().unwrap())?
                    <= timestamp(om["init_time"].as_str().unwrap())?
                {
                    anyhow::ensure!(
                        freshness(&om)?["available"] == true,
                        "No newer source and current model is stale"
                    );
                    event(
                        &root,
                        json!({"phase":"unchanged","model":model,"init_time":meta["init_time"]}),
                    )?;
                    return Ok::<_, anyhow::Error>(None);
                }
            }
            anyhow::ensure!(
                freshness(&meta)?["available"] == true,
                "Selected source is stale"
            );
            disk_guard(&root, admission(&model))?;
            event(
                &root,
                json!({"phase":"fetch","model":model,"revision":entry.revision,"init_time":meta["init_time"]}),
            )?;
            source::fetch(&model, &path, offset, Some(meta.clone()), false, None).await?;
            event(
                &root,
                json!({"phase":"staged","model":model,"revision":entry.revision}),
            )?;
            Ok(Some(meta))
        };
        let result = tokio::time::timeout(Duration::from_secs(2700), run).await;
        match result {
            Ok(Ok(Some(meta))) => return Ok(Some((path, entry, meta))),
            Ok(Ok(None)) => {
                let r = root.clone();
                tokio::task::spawn_blocking(move || store::discard(&r, &path)).await??;
                return Ok(None);
            }
            other => {
                let error = match other {
                    Ok(Err(e)) => format!("{e:#}"),
                    Err(_) => "Source extraction exceeded 45 minutes".into(),
                    _ => unreachable!(),
                };
                event(
                    &root,
                    json!({"phase":"failed","model":model,"revision":entry.revision,"error":error}),
                )?;
                atomic(
                    path.join("failed.json"),
                    &json!({"time":now(),"error":error}),
                )?;
                let r = root.clone();
                tokio::task::spawn_blocking(move || store::discard(&r, &path)).await??;
                errors.push(error);
            }
        }
    }
    anyhow::bail!("{}", errors.join("; "))
}
pub async fn update(
    root: PathBuf,
    models: Vec<String>,
    offsets: Vec<usize>,
    pin_root: Option<PathBuf>,
) -> Result<i32> {
    let _lock = Lock::acquire(&root, "update", true, true)?;
    let r = root.clone();
    tokio::task::spawn_blocking(move || -> Result<()> {
        store::collect(&r, 600.)?;
        let revisions = r.join("revisions");
        fs::create_dir_all(&revisions)?;
        for p in fs::read_dir(revisions)? {
            let p = p?.path();
            if p.is_dir() {
                store::discard(&r, &p)?;
            }
        }
        Ok(())
    })
    .await??;
    let started = now();
    let mut failures = vec![];
    let mut pending = None;
    let mut sources = SourceTasks::default();
    let result = async {
        for (i, model) in models.iter().enumerate() {
            let active = pending.take();
            if let Some(next) = models.get(i + 1) {
                if disk_guard(&root, admission(model) + admission(next)).is_ok() {
                    let pin = pin_root
                        .as_ref()
                        .map(|p| read_json(p.join(next).join("metadata.json")))
                        .transpose()?;
                    let task = tokio::spawn(prepare(
                        root.clone(),
                        next.clone(),
                        offsets.clone(),
                        pin,
                    ));
                    sources.track(&task);
                    pending = Some(task);
                } else {
                    event(
                        &root,
                        json!({"phase":"prefetch_deferred","model":next,"reason":"disk_budget"}),
                    )?;
                }
            }
            let prepared = if let Some(task) = active {
                task.await.context("Source worker failed")?
            } else {
                let pin = pin_root
                    .as_ref()
                    .map(|p| read_json(p.join(model).join("metadata.json")))
                    .transpose()?;
                prepare(root.clone(), model.clone(), offsets.clone(), pin).await
            };
            let (path, entry, meta) = match prepared {
                Ok(Some(v)) => v,
                Ok(None) => continue,
                Err(e) => {
                    failures.push(json!({"model":model,"error":format!("{e:#}")}));
                    continue;
                }
            };
            event(
                &root,
                json!({"phase":"load","model":model,"revision":entry.revision}),
            )?;
            let r = root.clone();
            let p = path.clone();
            let e = entry.clone();
            let name = model.clone();
            let result = tokio::task::spawn_blocking(move || -> Result<u64> {
                store::load(&r, &p, &e.table)?;
                let c = publish(&r, &name, e, false)?;
                clean_workspace(&p)?;
                Ok(c.generation)
            })
            .await?;
            match result {
                Ok(generation) => event(
                    &root,
                    json!({"phase":"published","model":model,"revision":entry.revision,"init_time":meta["init_time"],"generation":generation}),
                )?,
                Err(e) => {
                    let error = format!("{e:#}");
                    atomic(
                        path.join("failed.json"),
                        &json!({"time":now(),"error":error}),
                    )?;
                    event(
                        &root,
                        json!({"phase":"failed","model":model,"revision":entry.revision,"error":error}),
                    )?;
                    let r = root.clone();
                    tokio::task::spawn_blocking(move || store::discard(&r, &path)).await??;
                    failures.push(json!({"model":model,"error":error}));
                }
            }
        }
        Ok::<_,anyhow::Error>(())
    }
    .await;
    // Error paths must not detach an extractor and release update.lock while
    // it can still write staging. Abort and drain every tracked source first.
    sources.drain().await;
    result?;
    let r = root.clone();
    tokio::task::spawn_blocking(move || store::collect(&r, 600.)).await??;
    atomic(
        root.join("last-update.json"),
        &json!({"started":started,"time":now(),"failures":failures,"prefetch":true,"implementation":"rust"}),
    )?;
    Ok(if failures.is_empty() { 0 } else { 1 })
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
        models.insert(name.clone(),json!({"revision":e.revision,"table":e.table,"storage":m.get("storage"),"freshness":freshness(&m)?,"storage_rows":db.scalar(&format!("SELECT count() FROM {}.{}",ident(&e.database)?,ident(&e.table)?))?}));
    }
    Ok(json!({"catalog":c,"models":models}))
}
