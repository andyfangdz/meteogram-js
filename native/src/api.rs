use crate::{db::Db, grid, state::*};
use anyhow::{Context, Result};
use axum::{
    extract::{Query, Request, State},
    http::{HeaderMap, StatusCode},
    middleware::{self, Next},
    response::{IntoResponse, Response},
    routing::get,
    Json, Router,
};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, HashMap},
    io::Write,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};

pub fn api_token(root: &Path) -> Result<Option<String>> {
    let token = if let Ok(value) = std::env::var("METEOGRAM_API_TOKEN") {
        Some(value)
    } else if let Some(path) = std::env::var_os("METEOGRAM_API_TOKEN_FILE") {
        Some(std::fs::read_to_string(path).context("Read API token file")?)
    } else {
        let path = root.join("api-token");
        if path.exists() {
            Some(std::fs::read_to_string(path).context("Read API token file")?)
        } else {
            None
        }
    };
    token
        .map(|s| {
            let s = s.trim().to_owned();
            anyhow::ensure!(
                (32..=512).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_graphic()),
                "API token must contain 32–512 printable ASCII characters"
            );
            Ok(s)
        })
        .transpose()
}
pub fn client_headers(root: &Path) -> Result<HeaderMap> {
    let mut headers = HeaderMap::new();
    if let Some(token) = api_token(root)? {
        let mut value = format!("Bearer {token}").parse::<axum::http::HeaderValue>()?;
        value.set_sensitive(true);
        headers.insert(axum::http::header::AUTHORIZATION, value);
    }
    Ok(headers)
}
async fn authenticate(State(expected): State<[u8; 32]>, request: Request, next: Next) -> Response {
    let valid = request
        .headers()
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|h| h.to_str().ok())
        .and_then(|h| h.strip_prefix("Bearer "))
        .is_some_and(|token| Sha256::digest(token.as_bytes()).as_slice() == expected);
    if !valid {
        return (
            StatusCode::UNAUTHORIZED,
            [("www-authenticate", "Bearer")],
            Json(json!({"error":"Unauthorized"})),
        )
            .into_response();
    }
    next.run(request).await
}
struct Model {
    source_providers: Vec<&'static str>,
    meta: Value,
    ids: Vec<u32>,
    lat: Vec<f64>,
    lon: Vec<f64>,
    books: BTreeMap<u16, Arc<crate::dictionary::Dictionary>>,
    clouds: BTreeMap<u16, crate::clouds::Reader>,
}
#[derive(Clone)]
struct App {
    root: PathBuf,
    db: Db,
    cache: Arc<Mutex<HashMap<String, Arc<Model>>>>,
    pool: Arc<tokio::sync::Semaphore>,
    books: crate::runs::BookCache,
}
fn retain_models(cache: &mut HashMap<String, Arc<Model>>, c: &Catalog) {
    cache.retain(|revision, _| {
        c.current
            .values()
            .chain(c.previous.values())
            .chain(c.complete_runs.values().flatten())
            .chain(c.hrrr_long.iter())
            .any(|entry| &entry.revision == revision)
    });
}
fn model(root: &Path, e: &Entry, cache: &crate::runs::BookCache) -> Result<Model> {
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
    let coords = |n: &str, a: &mut BTreeMap<String, crate::stage::CellArray>| -> Result<Vec<f64>> {
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
    let books = crate::runs::load_books(root, e, &meta, Some(cache))?;
    let clouds = crate::clouds::readers(root, &meta)?;
    let mut source_providers = std::collections::BTreeSet::new();
    for book in crate::runs::books(&meta)? {
        let (_, _, child) = crate::runs::book_entry(root, &book)?;
        source_providers.insert(if child["source_provider"] == crate::ecmwf::PROVIDER {
            "ecmwf"
        } else {
            "dynamical"
        });
    }
    Ok(Model {
        source_providers: source_providers.into_iter().collect(),
        clouds,
        meta,
        ids,
        lat,
        lon,
        books,
    })
}
fn forecast(app: &App, q: HashMap<String, String>) -> (StatusCode, Value) {
    let parse = || -> Result<(String, f64, f64)> {
        let name = q.get("model").context("Missing model")?.clone();
        let lat: f64 = q.get("lat").context("Missing latitude")?.parse()?;
        let lon: f64 = q.get("lon").context("Missing longitude")?.parse()?;
        anyhow::ensure!(
            MODELS.contains(&name.as_str())
                && matches!(
                    q.get("run").map(String::as_str).unwrap_or("latest"),
                    "latest" | "latest_complete" | "previous_complete" | "long"
                )
                && (q.get("run").is_none_or(|v| v != "long") || name == "hrrr")
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
        let e = match q.get("run").map(String::as_str).unwrap_or("latest") {
            "latest_complete" => c.complete_runs.get(&name).and_then(|v| v.first()),
            "previous_complete" => c.complete_runs.get(&name).and_then(|v| v.get(1)),
            "long" => c.hrrr_long.as_ref(),
            _ => c.current.get(&name),
        }
        .context("Requested forecast run unavailable")?;
        let m = {
            let mut cache = app.cache.lock().unwrap();
            retain_models(&mut cache, &c);
            if !cache.contains_key(&e.revision) {
                cache.insert(
                    e.revision.clone(),
                    Arc::new(model(&app.root, e, &app.books)?),
                );
            }
            cache[&e.revision].clone()
        };
        let freshness = freshness(&m.meta)?;
        if q.get("run").is_none_or(|r| r == "latest") {
            anyhow::ensure!(freshness["available"] == true, "Stale model");
        }
        let (cell, i) = match grid::resolve(&m.meta, &m.ids, lat, lon) {
            Ok(v) => v,
            Err(e) => return Ok((StatusCode::NOT_FOUND, json!({"error":e.to_string()}))),
        };
        let (rows, elapsed) =
            app.db
                .profile_leads(&e.database, &e.table, &m.meta, cell, i, &m.books)?;
        let mut fields = vec!["lead_hour".to_owned()];
        fields.extend(crate::state::fields(&m.meta));
        let expected = expected_leads(&m.meta)?;
        let have: Vec<u16> = serde_json::from_value(m.meta["leads"].clone())?;
        let missing = expected
            .iter()
            .filter(|h| !have.contains(h))
            .copied()
            .collect::<Vec<_>>();
        let mut payload = json!({"model":name,"dataset":m.meta["dataset"],"init_time":m.meta["init_time"],"snapshot_id":m.meta["snapshot_id"],"requested":{"latitude":lat,"longitude":lon},"grid":{"cell_id":cell,"latitude":m.lat[i],"longitude":m.lon[i]},"levels_hpa":m.meta["levels"],"units":m.meta["units"],"fields":fields,"rows":rows,"native_wind_frame":if grid_relative(&name){"grid-relative"}else{"earth-relative"},"query_ms":elapsed,"revision":e.revision,"forecast_complete":missing.is_empty(),"available_leads":have,"expected_leads":expected,"missing_leads":missing});
        payload["source_providers"] = json!(m.source_providers);
        payload["lead_sources"] = json!(crate::runs::books(&m.meta)?
            .iter()
            .map(|b| json!({"lead":b.lead,"snapshot_id":b.snapshot_id}))
            .collect::<Vec<_>>());
        if let Some(clouds) = crate::clouds::payload(&m.clouds, i, &payload)? {
            payload["native_clouds"] = clouds;
        }
        payload["freshness"] = freshness;
        payload["wind_rotation_degrees"] = json!(if grid_relative(&name) {
            grid::Lambert::from_crs(&m.meta["grid"]["crs"])?.convergence_degrees(m.lon[i])
        } else {
            0.
        });
        Ok((StatusCode::OK, payload))
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
            for (name, e) in &c.current {
                let mut status =
                    freshness(&read_json(descriptor(&app.root, e)?.join("metadata.json"))?)?;
                let count = c.complete_runs.get(name).map_or(0, Vec::len);
                status["complete_runs_retained"] = json!(count);
                status["retention_satisfied"] =
                    json!(count >= 2 && (name != "hrrr" || c.hrrr_long.is_some()));
                if name == "hrrr" {
                    status["long_run_retained"] = json!(c.hrrr_long.is_some());
                }
                models.insert(name.clone(), status);
            }
            Ok(json!({"models":models,"native_fields":true}))
        };
        match run() {
            Ok(v) => {
                let good = MODELS
                    .iter()
                    .all(|name| v["models"][*name]["available"] == true);
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
    let token = api_token(&root)?;
    let db = tokio::task::spawn_blocking(Db::new).await??;
    let app = App {
        root,
        db,
        cache: Default::default(),
        pool: Arc::new(tokio::sync::Semaphore::new(4)),
        books: Default::default(),
    };
    let mut router = Router::new()
        .route("/forecast", get(handle))
        .route("/health", get(health))
        .route("/runs", get(list_runs))
        .route("/ingestion", get(ingestion_status))
        .with_state(app);
    if let Some(token) = token {
        let digest: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        router = router.layer(middleware::from_fn_with_state(digest, authenticate));
    }
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, port)).await?;
    println!("Rust native forecast API listening on 127.0.0.1:{port}");
    axum::serve(listener, router).await?;
    Ok(())
}
async fn ingestion_status(State(app): State<App>) -> (StatusCode, Json<Value>) {
    tokio::task::spawn_blocking(move || match crate::ingestion::snapshot(&app.root) {
        Ok(value) => (StatusCode::OK, Json(value)),
        Err(_) => (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error":"Ingestion status unavailable"})),
        ),
    })
    .await
    .unwrap_or((
        StatusCode::SERVICE_UNAVAILABLE,
        Json(json!({"error":"Ingestion status unavailable"})),
    ))
}
async fn list_runs(
    State(app): State<App>,
    Query(q): Query<HashMap<String, String>>,
) -> (StatusCode, Json<Value>) {
    let Some(name) = q
        .get("model")
        .filter(|n| MODELS.contains(&n.as_str()))
        .cloned()
    else {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({"error":"Invalid model"})),
        );
    };
    tokio::task::spawn_blocking(move|| {
        let run=||->Result<Value> {
            let _lock=Lock::acquire(&app.root,"catalog",false,false)?;
            let c=catalog(&app.root)?;
            let latest=c.current.get(&name).map(|e|crate::runs::summary(&app.root,e)).transpose()?;
            let complete=c.complete_runs.get(&name).into_iter().flatten().map(|e|crate::runs::summary(&app.root,e)).collect::<Result<Vec<_>>>()?;
            let building=c.building.get(&name).into_iter().flatten().map(|e|crate::runs::summary(&app.root,e)).collect::<Result<Vec<_>>>()?;
            let long=if name=="hrrr" {c.hrrr_long.as_ref().map(|e|crate::runs::summary(&app.root,e)).transpose()?}else{None};
            Ok(json!({"model":name,"latest":latest,"complete_runs":complete,"building":building,"long_run":long}))
        };
        match run() {Ok(v)=>(StatusCode::OK,Json(v)),Err(_)=>(StatusCode::SERVICE_UNAVAILABLE,Json(json!({"error":"Run inventory unavailable"})))}
    }).await.unwrap()
}
#[cfg(test)]
pub(crate) fn test_forecast(root: &Path, query: HashMap<String, String>) -> (StatusCode, Value) {
    forecast(
        &App {
            root: root.to_owned(),
            db: Db::new().unwrap(),
            cache: Default::default(),
            pool: Arc::new(tokio::sync::Semaphore::new(4)),
            books: Default::default(),
        },
        query,
    )
}

#[cfg(test)]
mod auth_tests {
    use super::*;

    #[test]
    fn selected_archive_models_survive_requests_until_catalog_eviction() {
        let entry = |revision: &str| Entry {
            revision: revision.to_owned(),
            database: "test".to_owned(),
            table: "test".to_owned(),
            owned: false,
            retired_at: None,
        };
        let mut catalog = Catalog::default();
        catalog.current.insert("hrrr".into(), entry("current"));
        catalog.previous.insert("hrrr".into(), entry("previous"));
        catalog
            .complete_runs
            .insert("hrrr".into(), vec![entry("complete")]);
        catalog.hrrr_long = Some(entry("long"));
        let mut cache = HashMap::new();
        for revision in ["current", "previous", "complete", "long", "obsolete"] {
            cache.insert(
                revision.to_owned(),
                Arc::new(Model {
                    source_providers: vec![],
                    meta: json!({}),
                    ids: vec![],
                    lat: vec![],
                    lon: vec![],
                    books: BTreeMap::new(),
                    clouds: BTreeMap::new(),
                }),
            );
        }
        let long = Arc::downgrade(&cache["long"]);
        for _ in 0..3 {
            retain_models(&mut cache, &catalog);
            assert_eq!(cache.len(), 4);
            assert!(Arc::ptr_eq(&cache["long"], &long.upgrade().unwrap()));
        }
        catalog.hrrr_long = None;
        catalog.complete_runs.clear();
        retain_models(&mut cache, &catalog);
        assert_eq!(cache.len(), 2);
        assert!(long.upgrade().is_none());
        assert!(!cache.contains_key("complete"));
    }

    #[tokio::test]
    async fn bearer_auth_protects_all_routes_before_handlers() {
        let token = "test-only-token-with-at-least-32-characters";
        let hash: [u8; 32] = Sha256::digest(token.as_bytes()).into();
        let mut router = Router::new();
        for path in ["/health", "/forecast", "/runs", "/ingestion"] {
            router = router.route(path, get(|| async { StatusCode::OK }));
        }
        let router = router.layer(middleware::from_fn_with_state(hash, authenticate));
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
        let client = reqwest::Client::builder().no_proxy().build().unwrap();
        for path in ["/health", "/forecast", "/runs", "/ingestion"] {
            let url = format!("http://127.0.0.1:{port}{path}");
            for value in [None, Some("Bearer wrong"), Some("Basic invalid")] {
                let mut request = client.get(&url);
                if let Some(value) = value {
                    request = request.header("authorization", value);
                }
                let response = request.send().await.unwrap();
                assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
                assert_eq!(response.headers()["www-authenticate"], "Bearer");
            }
            assert_eq!(
                client
                    .get(url)
                    .bearer_auth(token)
                    .send()
                    .await
                    .unwrap()
                    .status(),
                StatusCode::OK
            );
        }
        task.abort();
    }

    #[test]
    fn token_files_fail_closed_and_client_headers_hide_secrets() {
        let root = tempfile::tempdir().unwrap();
        assert!(api_token(root.path()).unwrap().is_none());
        std::fs::write(root.path().join("api-token"), "too-short").unwrap();
        assert!(api_token(root.path()).is_err());
        std::fs::write(
            root.path().join("api-token"),
            "public-test-token-longer-than-thirty-two-characters\n",
        )
        .unwrap();
        let headers = client_headers(root.path()).unwrap();
        assert!(headers["authorization"].is_sensitive());
        assert!(headers["authorization"]
            .to_str()
            .unwrap()
            .starts_with("Bearer "));
    }
}
