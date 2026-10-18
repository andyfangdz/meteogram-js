"""Run actual CONUS volumes with bounded disk use, preserving measurement evidence.

HRRR is split into eight disjoint spatial batches, each with all 49 times. Each
batch is loaded, verified, measured, then dropped. Source GRIB chunks are cached
once. This is a full-volume experiment, not a full-resident HRRR database test.
"""

import argparse
import fcntl
import hashlib
import json
import shutil
import subprocess
import sys
import time
from pathlib import Path

import numpy as np
import psutil
from benchmark import DATABASE, client

HERE = Path(__file__).resolve().parent


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--root", type=Path, required=True)
    parser.add_argument("--server-pid", type=int, required=True)
    parser.add_argument("--hrrr-pin", type=Path, required=True)
    parser.add_argument("--models", default="hrrr,gfs,aifs")
    parser.add_argument("--discard-completed-staging", action="store_true")
    args = parser.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    log_path = root / "operations.jsonl"
    connection = client()

    def record(data):
        with log_path.open("a") as output:
            output.write(json.dumps(data) + "\n")
        print(json.dumps(data), flush=True)

    def command(name, argv):
        started = time.perf_counter()
        record(
            {
                "phase": name,
                "status": "started",
                "utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            }
        )
        memory_peak = 0
        free_min = shutil.disk_usage(root).free
        with (root / f"{name}.log").open("a") as log:
            process = subprocess.Popen(
                [sys.executable, *map(str, argv)], stdout=log, stderr=subprocess.STDOUT
            )
            while process.poll() is None:
                memory_peak = max(
                    memory_peak, int(Path("/sys/fs/cgroup/memory.current").read_text())
                )
                free_min = min(free_min, shutil.disk_usage(root).free)
                time.sleep(1)
        row = {
            "phase": name,
            "status": "finished",
            "returncode": process.returncode,
            "seconds": time.perf_counter() - started,
            "peak_cgroup_memory_bytes": memory_peak,
            "minimum_free_disk_bytes": free_min,
            "server_rss_bytes": psutil.Process(args.server_pid).memory_info().rss,
        }
        record(row)
        if process.returncode:
            raise RuntimeError(f"{name} failed; see {root / f'{name}.log'}")

    for model in args.models.split(","):
        if model not in ("hrrr", "gfs", "aifs"):
            raise ValueError(model)
        shards = 8 if model == "hrrr" else 1
        leads = (
            list(range(49))
            if model == "hrrr"
            else list(range(121)) + list(range(123, 181, 3))
            if model == "gfs"
            else list(range(0, 181, 6))
        )
        for shard in range(shards):
            name = f"{model}-{shard}"
            data = root / name
            done = root / f"{name}-complete.json"
            if done.exists():
                record({"phase": name, "status": "already_verified"})
                continue
            data.mkdir(exist_ok=True)
            # A manually started acquisition may already be filling the same dir.
            with (data / ".fetch.lock").open("a") as lock:
                fcntl.flock(lock, fcntl.LOCK_EX)
            fetch = [
                HERE / "fetch.py",
                "--model",
                model,
                "--output",
                data,
                "--leads",
                ",".join(map(str, leads)),
                "--coverage",
                "conus",
                "--spatial-shards",
                shards,
                "--shard-index",
                shard,
                "--workers",
                16,
                "--concurrency",
                16,
            ]
            if model == "hrrr":
                fetch += [
                    "--pin-metadata",
                    args.hrrr_pin,
                    "--cache-dir",
                    root / "source-cache",
                ]
            for attempt in range(5):
                try:
                    command(f"{name}-fetch", fetch)
                    break
                except RuntimeError:
                    if attempt == 4:
                        raise
                    record(
                        {
                            "phase": f"{name}-fetch",
                            "status": "retrying",
                            "attempt": attempt + 2,
                        }
                    )
                    time.sleep(min(30, 5 * (attempt + 1)))
            command(
                f"{name}-staging",
                [
                    HERE / "measure-staging.py",
                    "--data",
                    data,
                    "--output",
                    root / f"{name}-staging.json",
                    "--layouts",
                    "by_field_4096_cell_blocks",
                ],
            )
            command(
                f"{name}-benchmark",
                [
                    HERE / "benchmark.py",
                    "--data",
                    data,
                    "--output",
                    root / f"{name}-benchmark.json",
                    "--server-pid",
                    args.server_pid,
                    "--layouts",
                    "256_zstd",
                    "--batch-cells",
                    1024 if model == "gfs" else 4096,
                    "--queries",
                    120,
                    "--read-during-ingest",
                ],
            )
            metadata = json.loads((data / "metadata.json").read_text())
            reports = [
                json.loads((data / f"lead-{lead:03d}.json").read_text())
                for lead in leads
            ]
            ids = np.load(data / "cells.npz")["cell_id"]
            if len(ids) != metadata["cells"] or np.any(
                np.diff(ids.astype(np.int64)) <= 0
            ):
                raise ValueError("Spatial cells are missing, duplicated or unordered")
            evidence = {
                "metadata": metadata,
                "source_reports": reports,
                "cell_id_min": int(ids.min()),
                "cell_id_max": int(ids.max()),
                "cell_ids_sha256": hashlib.sha256(ids.tobytes()).hexdigest(),
                "benchmark": json.loads((root / f"{name}-benchmark.json").read_text()),
                "staging": json.loads((root / f"{name}-staging.json").read_text()),
            }
            temporary = done.with_suffix(".tmp")
            temporary.write_text(json.dumps(evidence, indent=2))
            temporary.replace(done)
            if args.discard_completed_staging:
                for lead in leads:
                    (data / f"lead-{lead:03d}.npy").unlink()
                    (data / f"surface-{lead:03d}.npy").unlink()
            # Keep complete GFS/AIFS tables and the last HRRR batch for API tests.
            if model == "hrrr" and shard < shards - 1:
                connection.command(f"DROP TABLE {DATABASE}.hrrr_256_zstd SYNC")
            record(
                {
                    "phase": name,
                    "status": "verified",
                    "cells": len(ids),
                    "leads": len(leads),
                    "rows": len(ids) * len(leads),
                }
            )
    if "hrrr" in args.models.split(","):
        parts = [np.load(root / f"hrrr-{i}" / "cells.npz")["cell_id"] for i in range(8)]
        all_ids = np.concatenate(parts)
        expected = json.loads((root / "hrrr-0" / "metadata.json").read_text())["grid"][
            "conus_cells"
        ]
        if len(all_ids) != expected or np.any(np.diff(all_ids.astype(np.int64)) <= 0):
            raise ValueError(
                "HRRR shards overlap or have an incorrect total cell count"
            )
        record(
            {
                "phase": "coverage",
                "status": "verified",
                "unique_hrrr_cells": len(all_ids),
                "hrrr_rows": len(all_ids) * 49,
            }
        )


if __name__ == "__main__":
    main()
