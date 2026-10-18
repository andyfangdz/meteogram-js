use crate::{db::Db, grid, state::*};
use anyhow::{Context, Result};
use axum::{
    extract::{Query, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use std::{
    collections::{BTreeMap, HashMap},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
struct Model {
    meta: Value,
    ids: Vec<u32>,
    lat: Vec<f64>,
    lon: Vec<f64>,
}
#[derive(Clone)]
struct App {
    root: PathBuf,
    db: Db,
    cache: Arc<Mutex<HashMap<String, Arc<Model>>>>,
    pool: Arc<tokio::sync::Semaphore>,
}
fn model(root: &Path, e: &Entry) -> Result<Model> {
    let p = descriptor(root, e)?;
    let meta = read_json(p.join("metadata.json"))?;
    let mut arrays = crate::stage::load_cells(&p)?;
    let ids = arrays.remove("cell_id").context("Missing ids")?;
    anyhow::ensure!(
        ids.dtype == "<u4" && ids.bytes.len() % 4 == 0,
        "Invalid cell ids"
    );
    let ids = ids
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .map(|b| u32::from_le_bytes(*b))
        .collect::<Vec<_>>();
    let coords = |n: &str, a: &mut BTreeMap<String, Npy>| -> Result<Vec<f64>> {
        let v = a.remove(n).context("Missing coordinates")?;
        anyhow::ensure!(
            v.dtype == "<f8" && v.bytes.len() == ids.len() * 8,
            "Invalid coordinates"
        );
        Ok(v.bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|b| f64::from_le_bytes(*b))
            .collect())
    };
    let lat = coords("latitude", &mut arrays)?;
    let lon = coords("longitude", &mut arrays)?;
    anyhow::ensure!(ids.windows(2).all(|w| w[0] < w[1]), "Unsorted cell ids");
    Ok(Model {
        meta,
        ids,
        lat,
        lon,
    })
}
fn forecast(app: &App, q: HashMap<String, String>) -> (StatusCode, Value) {
    let parse = || -> Result<(String, f64, f64)> {
        let name = q.get("model").context("Missing model")?.clone();
        let lat: f64 = q.get("lat").context("Missing latitude")?.parse()?;
        let lon: f64 = q.get("lon").context("Missing longitude")?.parse()?;
        anyhow::ensure!(
            MODELS.contains(&name.as_str())
                && lat.is_finite()
                && lon.is_finite()
                && (-90. ..=90.).contains(&lat)
                && (-180. ..=180.).contains(&lon),
            "Invalid model or coordinates"
        );
        Ok((name, lat, lon))
    };
    let (name, lat, lon) = match parse() {
        Ok(v) => v,
        Err(e) => return (StatusCode::BAD_REQUEST, json!({"error":e.to_string()})),
    };
    let run = || -> Result<(StatusCode, Value)> {
        let _lock = Lock::acquire(&app.root, "catalog", false, false)?;
        let c = catalog(&app.root)?;
        let e = c.current.get(&name).context("Model unavailable")?;
        let m = {
            let mut cache = app.cache.lock().unwrap();
            if !cache.contains_key(&e.revision) {
                cache.insert(e.revision.clone(), Arc::new(model(&app.root, e)?));
            }
            let m = cache[&e.revision].clone();
            cache.retain(|r, _| c.current.values().any(|e| &e.revision == r));
            m
        };
        anyhow::ensure!(freshness(&m.meta)?["available"] == true, "Stale model");
        let (cell, i) = match grid::resolve(&m.meta, &m.ids, lat, lon) {
            Ok(v) => v,
            Err(e) => return Ok((StatusCode::NOT_FOUND, json!({"error":e.to_string()}))),
        };
        let (rows, elapsed) = app.db.profile(&e.database, &e.table, &m.meta, cell)?;
        let mut fields = vec!["lead_hour".to_owned()];
        fields.extend(crate::state::fields(&m.meta));
        Ok((
            StatusCode::OK,
            json!({"model":name,"dataset":m.meta["dataset"],"init_time":m.meta["init_time"],"snapshot_id":m.meta["snapshot_id"],"requested":{"latitude":lat,"longitude":lon},"grid":{"cell_id":cell,"latitude":m.lat[i],"longitude":m.lon[i]},"levels_hpa":m.meta["levels"],"units":m.meta["units"],"fields":fields,"rows":rows,"native_wind_frame":if name=="hrrr"{"grid-relative"}else{"earth-relative"},"query_ms":elapsed,"prototype":true,"revision":e.revision}),
        ))
    };
    run().unwrap_or((
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error":"Local native forecast unavailable or stale"}),
    ))
}
fn respond(status: StatusCode, body: Value, headers: &HeaderMap) -> Response {
    let raw = serde_json::to_vec(&body).unwrap();
    let gzip = headers
        .get("accept-encoding")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|s| s.trim().split(';').next() == Some("gzip"))
        });
    let mut h = HeaderMap::new();
    h.insert("content-type", "application/json".parse().unwrap());
    h.insert("cache-control", "no-store".parse().unwrap());
    if gzip {
        let mut z = flate2::write::GzEncoder::new(vec![], flate2::Compression::new(6));
        z.write_all(&raw).unwrap();
        h.insert("content-encoding", "gzip".parse().unwrap());
        (status, h, z.finish().unwrap()).into_response()
    } else {
        (status, h, raw).into_response()
    }
}
async fn handle(
    State(app): State<App>,
    Query(q): Query<HashMap<String, String>>,
    headers: HeaderMap,
) -> Response {
    let permit = app.pool.clone().acquire_owned().await.unwrap();
    let (status, body) = tokio::task::spawn_blocking(move || {
        let _p = permit;
        forecast(&app, q)
    })
    .await
    .unwrap_or((
        StatusCode::SERVICE_UNAVAILABLE,
        json!({"error":"Request failed"}),
    ));
    respond(status, body, &headers)
}
async fn health(State(app): State<App>) -> (StatusCode, Json<Value>) {
    tokio::task::spawn_blocking(move || {
        let run = || -> Result<Value> {
            let _l = Lock::acquire(&app.root, "catalog", false, false)?;
            let c = catalog(&app.root)?;
            app.db.command("SELECT 1")?;
            let mut models = serde_json::Map::new();
            for (name, e) in c.current {
                models.insert(
                    name,
                    freshness(&read_json(
                        descriptor(&app.root, &e)?.join("metadata.json"),
                    )?)?,
                );
            }
            Ok(json!({"models":models,"native_fields":true}))
        };
        match run() {
            Ok(v) => {
                let good = v["models"].as_object().unwrap().len() == 3
                    && v["models"]
                        .as_object()
                        .unwrap()
                        .values()
                        .all(|m| m["available"] == true);
                (
                    if good {
                        StatusCode::OK
                    } else {
                        StatusCode::SERVICE_UNAVAILABLE
                    },
                    Json(v),
                )
            }
            Err(_) => (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":"Native forecast service unavailable"})),
            ),
        }
    })
    .await
    .unwrap()
}
pub async fn serve(root: PathBuf, port: u16) -> Result<()> {
    let db = tokio::task::spawn_blocking(Db::new).await??;
    let app = App {
        root,
        db,
        cache: Default::default(),
        pool: Arc::new(tokio::sync::Semaphore::new(4)),
    };
    let router = Router::new()
        .route("/forecast", get(handle))
        .route("/health", get(health))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("Rust native forecast API listening on 127.0.0.1:{port}");
    axum::serve(listener, router).await?;
    Ok(())
}
