"""Run a single resident cycle with isolated server and disk-reserve telemetry."""

import argparse
import fcntl
import json
import os
import shutil
import socket
import subprocess
import sys
import threading
import time
import urllib.request
from pathlib import Path

import psutil


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--server-root", type=Path, required=True)
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument(
        "--models",
        default="hrrr,gfs,aifs",
        help="Models to (re)build; omitted models must already be verified in this experiment",
    )
    args = p.parse_args()
    models = args.models.split(",")
    if len(set(models)) != len(models) or any(
        m not in ("hrrr", "gfs", "aifs") for m in models
    ):
        p.error("models must be a nonempty, unique subset of hrrr,gfs,aifs")
    args.root.mkdir(parents=True, exist_ok=True)
    run_lock = (args.root / ".resident.lock").open("a")
    fcntl.flock(run_lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
    for port in (18123, 19000, 18080):
        with socket.socket() as sock:
            sock.settimeout(1)
            if sock.connect_ex(("127.0.0.1", port)) == 0:
                raise RuntimeError(f"Loopback port {port} is already occupied")
    complete = args.root / "complete.json"
    if complete.exists():
        complete.rename(args.root / f"complete-previous-{time.time_ns()}.json")
    scripts = Path(__file__).resolve().parent
    phase = "startup"
    stop = threading.Event()
    active = None
    failure = []
    started = time.time()
    host = {
        "started_unix": started,
        "uname": list(os.uname()),
        "cpu_count": psutil.cpu_count(),
        "memory": psutil.virtual_memory()._asdict(),
        "disk": shutil.disk_usage(args.root)._asdict(),
        "python": sys.version,
        "source_commit": subprocess.check_output(
            ["git", "rev-parse", "HEAD"], text=True
        ).strip(),
        "disk_counters": {
            k: v._asdict() for k, v in psutil.disk_io_counters(perdisk=True).items()
        },
    }
    host_path = args.root / "host.json"
    if host_path.exists():
        original_started = json.loads(host_path.read_text())["started_unix"]
        (args.root / f"host-restart-{time.time_ns()}.json").write_text(
            json.dumps(host, indent=2)
        )
    else:
        host_path.write_text(json.dumps(host, indent=2))
        original_started = started
    server_log = (args.root / "server-launch.log").open("a")
    server = subprocess.Popen(
        [
            sys.executable,
            str(scripts / "start-server.py"),
            "--binary",
            str(args.binary),
            "--work-dir",
            str(args.server_root),
        ],
        stdout=server_log,
        stderr=subprocess.STDOUT,
        env={**os.environ, "CLICKHOUSE_WATCHDOG_ENABLE": "0"},
    )

    def monitor():
        last_parts_sample = 0.0
        with (args.root / "telemetry.jsonl").open("a", buffering=1) as out:
            while not stop.is_set():
                disk = shutil.disk_usage(args.root)
                procs = []
                for proc in psutil.Process().children(recursive=True):
                    try:
                        procs.append(
                            {
                                "pid": proc.pid,
                                "name": proc.name(),
                                "rss": proc.memory_info().rss,
                                "cpu": sum(proc.cpu_times()[:2]),
                                "io": proc.io_counters()._asdict(),
                            }
                        )
                    except psutil.Error:
                        pass
                out.write(
                    json.dumps(
                        {
                            "unix": time.time(),
                            "phase": phase,
                            "server_attempt_started": started,
                            "free_bytes": disk.free,
                            "host_available_memory": psutil.virtual_memory().available,
                            "processes": procs,
                            "disk_write_bytes": {
                                k: v.write_bytes
                                for k, v in psutil.disk_io_counters(
                                    perdisk=True
                                ).items()
                            },
                        }
                    )
                    + "\n"
                )
                if time.time() - last_parts_sample >= 10:
                    last_parts_sample = time.time()
                    query = (
                        "SELECT table,active,count() AS parts,sum(bytes_on_disk) AS bytes "
                        "FROM system.parts WHERE database='meteogram_bench' "
                        "GROUP BY table,active ORDER BY table,active FORMAT JSONEachRow"
                    )
                    try:
                        request = urllib.request.Request(
                            "http://127.0.0.1:18123/", data=query.encode()
                        )
                        with urllib.request.urlopen(request, timeout=2) as response:
                            tables = [
                                json.loads(line)
                                for line in response.read().decode().splitlines()
                            ]
                        with (args.root / "parts-telemetry.jsonl").open("a") as parts:
                            parts.write(
                                json.dumps({"unix": time.time(), "tables": tables})
                                + "\n"
                            )
                    except OSError:
                        pass  # The server may still be starting.
                if disk.free < 20 * 1024**3:
                    failure.append("20 GiB free-space reserve reached")
                    if active is not None:
                        active.terminate()
                    break
                stop.wait(1)

    thread = threading.Thread(target=monitor, daemon=True)
    thread.start()

    def run(label, argv):
        nonlocal phase, active
        phase = label
        if failure:
            raise RuntimeError(failure[-1])
        log = args.root / f"{label}-{time.time_ns()}.log"
        before = time.time()
        with log.open("w") as out:
            active = subprocess.Popen(
                [sys.executable, "-u", *map(str, argv)],
                stdout=out,
                stderr=subprocess.STDOUT,
            )
            code = active.wait()
        record = {
            "phase": label,
            "elapsed_seconds": time.time() - before,
            "exit_code": code,
            "log": log.name,
        }
        with (args.root / "phases.jsonl").open("a") as out:
            out.write(json.dumps(record) + "\n")
        print(json.dumps(record), flush=True)
        active = None
        if code or failure:
            raise RuntimeError(f"Failed {label}; inspect {log}")

    try:
        for _ in range(60):
            if server.poll() is not None:
                raise RuntimeError("ClickHouse exited")
            try:
                with urllib.request.urlopen(
                    "http://127.0.0.1:18123/ping", timeout=1
                ) as r:
                    if r.read() == b"Ok.\n":
                        break
            except OSError:
                time.sleep(1)
        else:
            raise RuntimeError("ClickHouse readiness timeout")
        import numpy as np
        from benchmark import client, validate_keys

        conn = client()
        try:
            for model in {"hrrr", "gfs", "aifs"} - set(models):
                meta = json.loads((args.root / model / "metadata.json").read_text())
                prior = json.loads((args.root / f"{model}-benchmark.json").read_text())
                if (
                    "snapshot_id" in prior
                    and prior["snapshot_id"] != meta["snapshot_id"]
                ):
                    raise ValueError(f"Cannot resume mismatched {model} snapshot")
                for key in ("model", "init_time", "cells", "leads", "levels"):
                    if prior[key] != meta[key]:
                        raise ValueError(f"Cannot resume mismatched {model}: {key}")
                with np.load(args.root / model / "cells.npz") as cells:
                    check = validate_keys(
                        conn, f"{model}_256_zstd", cells["cell_id"], len(meta["leads"])
                    )
                print(
                    json.dumps({"phase": f"{model}-resume-validation", **check}),
                    flush=True,
                )
        finally:
            conn.close()
        for model in models:
            phase = f"{model}-wait-for-extractor"
            data = args.root / model
            data.mkdir(exist_ok=True)
            with (data / ".fetch.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
            leads = {
                "hrrr": "all",
                "gfs": ",".join(map(str, [*range(121), *range(123, 181, 3)])),
                "aifs": ",".join(map(str, range(0, 181, 6))),
            }[model]
            run(
                f"{model}-fetch",
                [
                    scripts / "fetch.py",
                    "--model",
                    model,
                    "--coverage",
                    "conus",
                    "--leads",
                    leads,
                    "--workers",
                    "16",
                    "--concurrency",
                    "16",
                    "--cache-dir",
                    args.root / "source-cache",
                    "--output",
                    data,
                ],
            )
            run(
                f"{model}-benchmark",
                [
                    scripts / "benchmark.py",
                    "--data",
                    data,
                    "--output",
                    args.root / f"{model}-benchmark.json",
                    "--server-pid",
                    server.pid,
                    "--layouts",
                    "256_zstd",
                    "--batch-cells",
                    "1024" if model == "gfs" else "4096",
                    "--queries",
                    "300",
                    "--read-during-ingest",
                ],
            )
        for model in ("hrrr", "gfs", "aifs"):
            run(
                f"{model}-staging",
                [
                    scripts / "measure-staging.py",
                    "--data",
                    args.root / model,
                    "--output",
                    args.root / f"{model}-staging.json",
                    "--layouts",
                    "by_field_4096_cell_blocks",
                ],
            )
        run(
            "api-verification",
            [
                scripts / "verify-api.py",
                "--data-root",
                args.root,
                "--output",
                args.root / "api-verification.json",
            ],
        )
        (args.root / "complete.json").write_text(
            json.dumps(
                {
                    "elapsed_seconds": time.time() - original_started,
                    "final_attempt_seconds": time.time() - started,
                    "completed_unix": time.time(),
                }
            )
        )
    finally:
        if active is not None and active.poll() is None:
            active.terminate()
            try:
                active.wait(timeout=10)
            except subprocess.TimeoutExpired:
                active.kill()
                active.wait()
        stop.set()
        thread.join()
        server.terminate()
        try:
            server.wait(timeout=30)
        except subprocess.TimeoutExpired:
            server.kill()


if __name__ == "__main__":
    main()
