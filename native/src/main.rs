mod api;
mod clouds;
mod db;
mod dictionary;
mod grid;
mod incremental;
mod observe;
mod packed;
mod runs;
mod service;
mod source;
mod stage;
mod state;
mod store;
mod verify;
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;
use std::path::PathBuf;
#[derive(Parser)]
#[command(
    version,
    about = "Local CONUS forecast ingestion, catalog, API and verification. Rust/Cargo only."
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}
#[derive(Subcommand)]
enum Cmd {
    InspectDictionary {
        #[arg(long)]
        data: PathBuf,
    },
    CloudAvailability {
        #[arg(long, default_value = "noaa-hrrr-forecast-18-hour-virtual")]
        product: String,
    },
    /// Add native clouds to immutable HRRR hourly descriptors without changing pressure data.
    NativeClouds {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "latest")]
        run: String,
        #[arg(long, value_delimiter = ',')]
        leads: Option<Vec<u16>>,
    },
    Inspect {
        model: String,
    },
    Probe {
        #[arg(long)]
        model: String,
        #[arg(long)]
        pin_metadata: PathBuf,
    },
    Observe {
        #[arg(long, default_value = "http://127.0.0.1:18080")]
        base_url: String,
        #[arg(long)]
        until: PathBuf,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        disk_root: PathBuf,
        #[arg(long, default_value_t = 3600)]
        max_seconds: u64,
    },
    Serve {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 18080)]
        port: u16,
    },
    Update {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "hrrr,gfs,aifs,rrfs")]
        models: Vec<String>,
        #[arg(long, default_value_t = 50)]
        work_seconds: u64,
    },
    Watch {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 60.)]
        poll_seconds: f64,
    },
    Availability {
        #[arg(long)]
        model: String,
        #[arg(long)]
        product: Option<String>,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Validate {
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        table: String,
        #[arg(long, default_value = "meteogram_native")]
        database: String,
        #[arg(long)]
        staged: bool,
    },
    Status {
        #[arg(long)]
        root: PathBuf,
    },
    Rollback {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        model: String,
    },
    Gc {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 600.)]
        grace_seconds: f64,
    },
    Stop {
        #[arg(long)]
        root: PathBuf,
    },
    Service {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        server_root: PathBuf,
        #[arg(long)]
        binary: PathBuf,
        #[arg(long, default_value_t = 60.)]
        poll_seconds: f64,
    },
    StartServer {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        work_dir: PathBuf,
    },
    Verify {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:18080")]
        base_url: String,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    CheckApi {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 18082)]
        port: u16,
        #[arg(long)]
        full: bool,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    Benchmark {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value_t = 300)]
        count: usize,
        #[arg(long, default_value_t = 1)]
        clients: usize,
        #[arg(long)]
        output: Option<PathBuf>,
    },
}
fn output(value: &serde_json::Value, path: Option<PathBuf>) -> Result<()> {
    if let Some(p) = path {
        state::atomic(p, value)?;
    }
    println!("{}", serde_json::to_string_pretty(value)?);
    Ok(())
}
#[tokio::main]
async fn main() -> Result<()> {
    match Cli::parse().command {
        Cmd::InspectDictionary { data } => output(&dictionary::inspect(&data)?, None)?,
        Cmd::Observe {
            base_url,
            until,
            output,
            disk_root,
            max_seconds,
        } => observe::observe(base_url, until, output, disk_root, max_seconds).await?,
        Cmd::Probe {
            model,
            pin_metadata,
        } => output(
            &source::probe(&model, &state::read_json(pin_metadata)?).await?,
            None,
        )?,
        Cmd::CloudAvailability { product } => {
            output(&source::cloud_availability(&product).await?, None)?
        }
        Cmd::NativeClouds { root, run, leads } => {
            output(&clouds::backfill(&root, &run, leads).await?, None)?
        }
        Cmd::Inspect { model } => {
            let s = source::Source::open(&model, None).await?;
            println!("snapshot={}", s.snapshot);
            for n in [
                "init_time",
                "lead_time",
                "pressure_level/pressure_level",
                "x",
                "y",
                "latitude",
                "longitude",
                "spatial_ref",
                "pressure_level/temperature",
                "model_level/model_level",
                "model_level/fraction_of_cloud_cover",
                "model_level/geopotential_height",
                "model_level/pressure",
            ] {
                match s.meta(n).await {
                    Ok(v) => println!("{n}: {v}"),
                    Err(e) => eprintln!("{n}: {e}"),
                }
            }
        }
        Cmd::Serve { root, port } => api::serve(root, port).await?,
        Cmd::Update {
            root,
            models,
            work_seconds,
        } => {
            anyhow::ensure!(
                models.iter().all(|s| state::MODELS.contains(&s.as_str())),
                "Invalid models"
            );
            let code = incremental::poll(root, models, work_seconds).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
        Cmd::Watch { root, poll_seconds } => incremental::watch(root, poll_seconds).await?,
        Cmd::Availability {
            model,
            product,
            output: path,
        } => {
            let product = product.unwrap_or(state::dataset(&model)?.to_owned());
            let result = source::availability(&model, &product, &[]).await?;
            output(
                &json!({"model":model,"dataset":product,"snapshot_id":result.source.snapshot,"payload_reads":0,"runs":result.runs.iter().map(|r|json!({"init_time":r.pin["init_time"],"expected_leads":r.expected,"ready_leads":r.ready,"missing_references":r.missing})).collect::<Vec<_>>()}),
                path,
            )?;
        }
        Cmd::Validate {
            data,
            table,
            database,
            staged,
        } => output(
            &tokio::task::spawn_blocking(move || store::validate(&data, &table, &database, staged))
                .await??,
            None,
        )?,
        Cmd::Status { root } => output(
            &tokio::task::spawn_blocking(move || runs::status(&root)).await??,
            None,
        )?,
        Cmd::Rollback { root, model } => {
            tokio::task::spawn_blocking(move || runs::rollback(&root, &model)).await??
        }
        Cmd::Gc {
            root,
            grace_seconds,
        } => {
            anyhow::ensure!(
                grace_seconds.is_finite() && grace_seconds >= 0.,
                "Invalid grace"
            );
            tokio::task::spawn_blocking(move || {
                let _l = state::Lock::acquire(&root, "update", true, true)?;
                store::collect(&root, grace_seconds)
            })
            .await??;
        }
        Cmd::Stop { root } => state::atomic(
            root.join("stop-request.json"),
            &json!({"requested_at":state::now()}),
        )?,
        Cmd::Service {
            root,
            server_root,
            binary,
            poll_seconds,
        } => service::service(root, server_root, binary, poll_seconds).await?,
        Cmd::StartServer { binary, work_dir } => service::start_server(&binary, &work_dir)?,
        Cmd::Verify {
            root,
            base_url,
            full,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || verify::verify(&root, &base_url, full)).await??,
            path,
        )?,
        Cmd::CheckApi {
            root,
            port,
            full,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || verify::check_api(&root, port, full)).await??,
            path,
        )?,
        Cmd::Benchmark {
            root,
            count,
            clients,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || verify::benchmark(&root, count, clients))
                .await??,
            path,
        )?,
    }
    Ok(())
}

#[cfg(test)]
mod tests;
