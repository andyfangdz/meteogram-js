use crate::state::*;
use anyhow::{bail, Context, Result};
use serde_json::json;
use std::{
    collections::HashMap,
    fs::{self, OpenOptions},
    net::{Ipv4Addr, SocketAddrV4, TcpStream},
    os::unix::process::CommandExt,
    path::{Path, PathBuf},
    process::{Child, Command, Stdio},
    time::Duration,
};
struct Children(HashMap<String, Child>);
impl Children {
    fn alive(&mut self, n: &str) -> bool {
        self.0
            .get_mut(n)
            .is_some_and(|c| c.try_wait().ok().flatten().is_none())
    }
    fn stop(&mut self, n: &str) {
        if let Some(mut child) = self.0.remove(n) {
            let pid = child.id() as i32;
            unsafe {
                libc::kill(-pid, libc::SIGTERM);
            }
            for _ in 0..300 {
                if child.try_wait().ok().flatten().is_some() {
                    return;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
            unsafe {
                libc::kill(-pid, libc::SIGKILL);
            }
            let _ = child.wait();
        }
    }
    fn start(&mut self, root: &Path, n: &str, cmd: &mut Command) -> Result<()> {
        self.stop(n);
        let p = root.join(format!("{n}.log"));
        if p.exists() && p.metadata()?.len() > 20 << 20 {
            fs::rename(&p, root.join(format!("{n}.previous.log")))?;
        }
        let log = OpenOptions::new().create(true).append(true).open(p)?;
        cmd.stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .env("CLICKHOUSE_WATCHDOG_ENABLE", "0");
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() < 0 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        self.0.insert(n.to_owned(), cmd.spawn()?);
        Ok(())
    }
}
impl Drop for Children {
    fn drop(&mut self) {
        for n in ["update", "api", "clickhouse"] {
            self.stop(n);
        }
    }
}
pub fn start_server(binary: &Path, root: &Path) -> Result<()> {
    for n in ["data", "tmp", "log"] {
        fs::create_dir_all(root.join("ch").join(n))?;
    }
    let cfg = include_str!("../server.xml")
        .replace("./ch/", &format!("{}/ch/", root.canonicalize()?.display()));
    fs::write(root.join("server.xml"), cfg)?;
    let error = Command::new(binary.canonicalize()?)
        .arg("server")
        .arg(format!(
            "--config-file={}",
            root.join("server.xml").display()
        ))
        .current_dir(root)
        .env("CLICKHOUSE_WATCHDOG_ENABLE", "0")
        .exec();
    Err(error.into())
}
pub async fn service(
    root: PathBuf,
    server_root: PathBuf,
    binary: PathBuf,
    interval: f64,
) -> Result<()> {
    anyhow::ensure!(interval.is_finite() && interval > 0., "Invalid interval");
    fs::create_dir_all(&root)?;
    let _lock = Lock::acquire(&root, "service", true, true)?;
    for p in [18123, 19000, 18080] {
        if TcpStream::connect_timeout(
            &SocketAddrV4::new(Ipv4Addr::LOCALHOST, p).into(),
            Duration::from_secs(1),
        )
        .is_ok()
        {
            bail!("Loopback port {p} already occupied");
        }
    }
    let exe = std::env::current_exe()?;
    let stop = root.join("stop-request.json");
    if stop.exists() {
        fs::remove_file(&stop)?;
    }
    let schedule = root.join("schedule.json");
    let mut next_update = if schedule.exists() {
        read_json(&schedule)?["next_update"].as_f64().unwrap_or(0.)
    } else {
        0.
    };
    let mut children = Children(HashMap::new());
    let http = reqwest::Client::builder()
        .no_proxy()
        .timeout(Duration::from_secs(2))
        .build()?;
    let mut term = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?;
    let result = async {
        loop {
            if stop.exists() {
                fs::remove_file(&stop)?;
                break;
            }
            if !children.alive("clickhouse") {
                children.stop("api");
                children.stop("update");
                children.start(
                    &root,
                    "clickhouse",
                    Command::new(&exe)
                        .arg("start-server")
                        .arg("--binary")
                        .arg(&binary)
                        .arg("--work-dir")
                        .arg(&server_root),
                )?;
                let mut ready = false;
                for _ in 0..60 {
                    if http
                        .get("http://127.0.0.1:18123/ping")
                        .send()
                        .await
                        .is_ok_and(|r| r.status().is_success())
                    {
                        ready = true;
                        break;
                    }
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
                anyhow::ensure!(ready, "ClickHouse failed to become ready");
            }
            if !children.alive("api") {
                children.start(
                    &root,
                    "api",
                    Command::new(&exe)
                        .arg("serve")
                        .arg("--root")
                        .arg(&root)
                        .arg("--port")
                        .arg("18080"),
                )?;
            }
            if children.0.contains_key("update") && !children.alive("update") {
                let mut child = children.0.remove("update").unwrap();
                let code = child.wait()?.code().unwrap_or(1);
                next_update = now() + if code == 0 { interval * 3600. } else { 1800. };
                atomic(
                    &schedule,
                    &json!({"next_update":next_update,"last_exit":code}),
                )?;
            }
            if now() >= next_update && !children.alive("update") {
                children.start(
                    &root,
                    "update",
                    Command::new(&exe).arg("update").arg("--root").arg(&root),
                )?;
            }
            let processes: serde_json::Map<String, serde_json::Value> = children
                .0
                .iter_mut()
                .map(|(n, c)| {
                    (
                        n.clone(),
                        json!({"pid":c.id(),
                    "running":c.try_wait().ok().flatten().is_none()}),
                    )
                })
                .collect();
            atomic(
                root.join("service-status.json"),
                &json!({"time":now(),
                "next_update":next_update,"children":processes,"implementation":"rust"}),
            )?;
            tokio::select! {
                _ = tokio::time::sleep(Duration::from_secs(2)) => {},
                _ = term.recv() => break,
                _ = interrupt.recv() => break,
            }
        }
        Ok::<_, anyhow::Error>(())
    }
    .await;
    let cleanup = tokio::task::spawn_blocking(move || drop(children)).await;
    let _ = atomic(
        root.join("service-status.json"),
        &json!({"time":now(),"stopped":true,"next_update":next_update,"implementation":"rust"}),
    );
    cleanup.context("Child cleanup failed")?;
    result
}
