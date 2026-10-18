mod api;
mod db;
mod dictionary;
mod grid;
mod observe;
mod pack;
mod packed;
mod pipeline;
mod prepare;
mod service;
mod source;
mod stage;
mod state;
mod store;
mod verify;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::{fs, path::PathBuf};
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
    SortStage {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    PrepareStore {
        #[arg(long)]
        source: PathBuf,
        #[arg(long, default_value = "meteogram_native")]
        database: String,
        #[arg(long)]
        table: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long)]
        raw_output: Option<PathBuf>,
    },
    InspectDictionary {
        #[arg(long)]
        data: PathBuf,
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
    Fetch {
        #[arg(long)]
        model: String,
        #[arg(long)]
        output: PathBuf,
        #[arg(long, default_value_t = 2)]
        run_offset: usize,
        #[arg(long)]
        metadata_only: bool,
        #[arg(long)]
        pin_metadata: Option<PathBuf>,
        #[arg(long, value_delimiter = ',')]
        leads: Option<Vec<u16>>,
    },
    Update {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, value_delimiter = ',', default_value = "hrrr,gfs,aifs")]
        models: Vec<String>,
        #[arg(long, value_delimiter = ',', default_value = "1,2")]
        offsets: Vec<usize>,
        #[arg(long)]
        pin_root: Option<PathBuf>,
    },
    Load {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        data: PathBuf,
        #[arg(long)]
        table: String,
        #[arg(long)]
        workers: Option<usize>,
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
        #[arg(long, default_value_t = 6.)]
        interval_hours: f64,
    },
    StartServer {
        #[arg(long)]
        binary: PathBuf,
        #[arg(long)]
        work_dir: PathBuf,
    },
    Adopt {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        database: String,
        #[arg(long)]
        table: String,
    },
    Verify {
        #[arg(long)]
        root: PathBuf,
        #[arg(long, default_value = "http://127.0.0.1:18080")]
        base_url: String,
        #[arg(long)]
        compare_url: Option<String>,
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
        compare_url: Option<String>,
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
    BenchmarkPack {
        #[arg(long, default_value_t = 4096)]
        cells: usize,
        #[arg(long, default_value_t = 49)]
        leads: usize,
        #[arg(long, default_value_t = 5)]
        repeats: usize,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    CompareStores {
        #[arg(long)]
        root: PathBuf,
        #[arg(long)]
        reference_root: PathBuf,
        #[arg(long, default_value_t = 100)]
        samples: usize,
        #[arg(long)]
        output: Option<PathBuf>,
    },
    ConvertStage {
        #[arg(long)]
        source: PathBuf,
        #[arg(long)]
        output: PathBuf,
    },
    StagingCompression {
        #[arg(long)]
        data: PathBuf,
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
        Cmd::SortStage {
            source,
            output: target,
        } => output(
            &tokio::task::spawn_blocking(move || prepare::sorted_stage(&source, &target)).await??,
            None,
        )?,
        Cmd::InspectDictionary { data } => output(&dictionary::inspect(&data)?, None)?,
        Cmd::PrepareStore {
            source,
            database,
            table,
            output: target,
            raw_output,
        } => output(
            &tokio::task::spawn_blocking(move || {
                prepare::from_store(&source, &database, &table, &target, raw_output.as_deref())
            })
            .await??,
            None,
        )?,
        Cmd::ConvertStage {
            source,
            output: target,
        } => output(&stage::convert(&source, &target)?, None)?,
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
            ] {
                match s.meta(n).await {
                    Ok(v) => println!("{n}: {v}"),
                    Err(e) => eprintln!("{n}: {e}"),
                }
            }
        }
        Cmd::Serve { root, port } => api::serve(root, port).await?,
        Cmd::Fetch {
            model,
            output,
            run_offset,
            metadata_only,
            pin_metadata,
            leads,
        } => {
            let pin = pin_metadata.map(state::read_json).transpose()?;
            source::fetch(&model, &output, run_offset, pin, metadata_only, leads).await?;
        }
        Cmd::Update {
            root,
            models,
            offsets,
            pin_root,
        } => {
            anyhow::ensure!(
                models.iter().all(|s| state::MODELS.contains(&s.as_str()))
                    && offsets.iter().all(|o| *o > 0),
                "Invalid models/offsets"
            );
            let code = pipeline::update(root, models, offsets, pin_root).await?;
            if code != 0 {
                std::process::exit(code);
            }
        }
        Cmd::Load {
            root,
            data,
            table,
            workers,
        } => output(
            &tokio::task::spawn_blocking(move || {
                store::load_with_workers(&root, &data, &table, workers)
            })
            .await??,
            None,
        )?,
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
            &tokio::task::spawn_blocking(move || pipeline::status(&root)).await??,
            None,
        )?,
        Cmd::Rollback { root, model } => {
            tokio::task::spawn_blocking(move || pipeline::rollback(&root, &model)).await??
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
            interval_hours,
        } => service::service(root, server_root, binary, interval_hours).await?,
        Cmd::StartServer { binary, work_dir } => service::start_server(&binary, &work_dir)?,
        Cmd::Adopt {
            root,
            source,
            database,
            table,
        } => {
            tokio::task::spawn_blocking(move || -> Result<()> {
                let _l = state::Lock::acquire(&root, "update", true, true)?;
                state::ident(&database)?;
                state::ident(&table)?;
                let m = state::read_json(source.join("metadata.json"))?;
                state::check_metadata(&m)?;
                let model = m["model"].as_str().context("Model")?;
                let (p, e) = state::candidate(&root, model)?;
                fs::copy(source.join("metadata.json"), p.join("metadata.json"))?;
                stage::copy_cells(&source, &p)?;
                dictionary::copy(&source, &p, &m)?;
                let db = db::Db::new()?;
                db.command("CREATE DATABASE IF NOT EXISTS meteogram_native")?;
                db.command(&format!(
                    "CREATE TABLE meteogram_native.{} CLONE AS {database}.{table}",
                    e.table
                ))?;
                db.command(&format!(
                    "ALTER TABLE meteogram_native.{} MODIFY SETTING old_parts_lifetime=60",
                    e.table
                ))?;
                store::validate(&p, &e.table, &e.database, false)?;
                state::publish(&root, model, e, false)?;
                Ok(())
            })
            .await??
        }
        Cmd::Verify {
            root,
            base_url,
            compare_url,
            full,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || {
                verify::verify(&root, &base_url, compare_url.as_deref(), full)
            })
            .await??,
            path,
        )?,
        Cmd::CheckApi {
            root,
            port,
            compare_url,
            full,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || {
                verify::check_api(&root, port, compare_url.as_deref(), full)
            })
            .await??,
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
        Cmd::CompareStores {
            root,
            reference_root,
            samples,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || {
                verify::compare_stores(&root, &reference_root, samples)
            })
            .await??,
            path,
        )?,
        Cmd::BenchmarkPack {
            cells,
            leads,
            repeats,
            output: path,
        } => output(
            &tokio::task::spawn_blocking(move || verify::benchmark_pack(cells, leads, repeats))
                .await??,
            path,
        )?,
        Cmd::StagingCompression { data, output: path } => output(
            &tokio::task::spawn_blocking(move || verify::staging(&data)).await??,
            path,
        )?,
    }
    Ok(())
}

#[cfg(test)]
mod tests;
