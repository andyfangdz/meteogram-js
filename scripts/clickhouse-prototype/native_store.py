"""Revision catalog, bounded loader and publication gates for the native service.

The catalog is the publication boundary. Readers hold a shared advisory lock
through their DB read; publication/GC hold the exclusive lock. A single updater
holds update.lock for its entire transaction, including crash recovery.
"""

import contextlib
import fcntl
import json
import os
import re
import shutil
import time
from datetime import UTC, datetime
from pathlib import Path

import numpy as np
import psutil
import pyarrow as pa
from benchmark import client, query_profile, validate_keys

DATABASE = "meteogram_native"
MODELS = ("hrrr", "gfs", "aifs")
LEADS = {
    "hrrr": list(range(49)),
    "gfs": list(range(121)) + list(range(123, 181, 3)),
    "aifs": list(range(0, 181, 6)),
}
POLICY = {
    "hrrr": {"max_age_hours": 24, "min_remaining_hours": 12},
    "gfs": {"max_age_hours": 48, "min_remaining_hours": 72},
    "aifs": {"max_age_hours": 48, "min_remaining_hours": 72},
}
RESERVE = 20 * 1024**3
# Extra free space required before starting a model, beyond the reserve.
# Includes source, raw staging, new table, merge output and obsolete parts.
ADMISSION = {"hrrr": 210 * 1024**3, "gfs": 35 * 1024**3, "aifs": 5 * 1024**3}


def identifier(value):
    if not re.fullmatch(r"[a-z][a-z0-9_]*", value):
        raise ValueError("Unsafe SQL identifier")
    return value


def timestamp(value):
    parsed = datetime.fromisoformat(value)
    return (parsed.replace(tzinfo=UTC) if parsed.tzinfo is None else parsed).timestamp()


def freshness(meta, now=None):
    now = time.time() if now is None else now
    age = (now - timestamp(meta["init_time"])) / 3600
    remaining = max(meta["leads"]) - age
    policy = POLICY[meta["model"]]
    return {
        "init_time": meta["init_time"],
        "available": -1 <= age <= policy["max_age_hours"]
        and remaining >= policy["min_remaining_hours"],
        "age_hours": round(age, 3),
        "remaining_hours": round(remaining, 3),
        **policy,
    }


def atomic_json(path, data):
    path = Path(path)
    path.parent.mkdir(parents=True, exist_ok=True)
    temporary = path.with_suffix(".tmp")
    with temporary.open("w") as out:
        json.dump(data, out, indent=2, allow_nan=False)
        out.flush()
        os.fsync(out.fileno())
    temporary.replace(path)
    fd = os.open(path.parent, os.O_RDONLY | os.O_DIRECTORY)
    try:
        os.fsync(fd)
    finally:
        os.close(fd)


@contextlib.contextmanager
def lock(root, name="catalog", exclusive=False, nonblocking=False):
    root = Path(root)
    root.mkdir(parents=True, exist_ok=True)
    with (root / f"{name}.lock").open("a") as handle:
        mode = fcntl.LOCK_EX if exclusive else fcntl.LOCK_SH
        fcntl.flock(handle, mode | (fcntl.LOCK_NB if nonblocking else 0))
        try:
            yield
        finally:
            fcntl.flock(handle, fcntl.LOCK_UN)


def catalog(root):
    path = Path(root) / "catalog.json"
    if path.exists():
        return json.loads(path.read_text())
    return {"version": 1, "generation": 0, "current": {}, "previous": {}, "retired": []}


def check_metadata(meta):
    model = meta["model"]
    levels = (
        [1000, 925, 850, 700, 600, 500, 400, 300]
        if model == "aifs"
        else list(range(1000, 274, -25))
    )
    variables = ["geopotential_height", "temperature", "wind_u", "wind_v"]
    if model == "gfs":
        variables = ["cloud_cover", *variables, "relative_humidity"]
    else:
        variables += (
            ["specific_humidity"]
            if model == "aifs"
            else ["relative_humidity", "dew_point_temperature"]
        )
    if (
        meta["coverage"] != "conus"
        or meta.get("spatial_shards", 1) != 1
        or meta["leads"] != LEADS[model]
        or meta["levels"] != levels
        or meta["variables"] != variables
        or meta["surface_variables"] != ["temperature_2m", "dew_point_temperature_2m"]
        or meta["cells"] != (1638757 if model == "hrrr" else 24885)
    ):
        raise ValueError("Source schema/coverage changed; inspect before publication")


def descriptor_path(root, entry):
    path = (Path(root) / "revisions" / entry["revision"]).resolve()
    if path.parent != (Path(root) / "revisions").resolve():
        raise ValueError("Revision outside managed directory")
    return path


def publish(root, model, entry, rollback=False):
    # Validation marker is written only after the full DB gates pass.
    path = descriptor_path(root, entry)
    validation = json.loads((path / "validated.json").read_text())
    if (
        validation["table"] != entry["table"]
        or validation["database"] != entry["database"]
    ):
        raise ValueError("Validation belongs to another table")
    meta = json.loads((path / "metadata.json").read_text())
    check_metadata(meta)
    if meta["model"] != model or any(
        validation[key] != meta[key] for key in ("init_time", "snapshot_id")
    ):
        raise ValueError("Validation does not match model metadata")
    if not freshness(meta)["available"]:
        raise ValueError("Forecast is too old or has insufficient remaining horizon")
    with lock(root, exclusive=True):
        state = catalog(root)
        current = state["current"].get(model)
        if current:
            old_meta = json.loads(
                (descriptor_path(root, current) / "metadata.json").read_text()
            )
            if not rollback and timestamp(meta["init_time"]) <= timestamp(
                old_meta["init_time"]
            ):
                raise ValueError("Publication must advance forecast initialization")
        previous = state["previous"].get(model)
        if previous and previous != entry:
            state["retired"].append({**previous, "retired_at": time.time()})
        state["current"][model] = entry
        if current:
            state["previous"][model] = current
        state["generation"] += 1
        state["published_at"] = time.time()
        atomic_json(Path(root) / "catalog.json", state)
    return state


def disk_guard(root, extra=0):
    free = shutil.disk_usage(root).free
    if free < RESERVE + extra:
        raise OSError(
            f"Insufficient disk: {free / 1024**3:.1f} GiB free; need {(RESERVE + extra) / 1024**3:.1f} GiB"
        )


def load(root, path, table):
    table = identifier(table)
    meta = json.loads((path / "metadata.json").read_text())
    leads = meta["leads"]
    if meta["coverage"] != "conus" or leads != LEADS[meta["model"]]:
        raise ValueError("Unexpected coverage or forecast cadence")
    ids = np.load(path / "cells.npz")["cell_id"]
    profiles, surfaces = [], []
    for lead in leads:
        report = json.loads((path / f"lead-{lead:03d}.json").read_text())
        if not all(s["finite"] == s["total"] for s in report["fields"].values()):
            raise ValueError("Missing source values: retain the last complete forecast")
        profiles.append(np.load(path / f"lead-{lead:03d}.npy", mmap_mode="r"))
        surfaces.append(np.load(path / f"surface-{lead:03d}.npy", mmap_mode="r"))
    fields = meta["variables"]
    ground_fields = meta["surface_variables"]
    levels = len(meta["levels"])
    started = time.perf_counter()
    timings = {"gather_arrow_seconds": 0.0, "insert_seconds": 0.0}
    process = psutil.Process()
    io_before = process.io_counters()
    c = client()
    try:
        c.command(f"CREATE DATABASE IF NOT EXISTS {DATABASE}")
        columns = ["revision_id UInt32", "cell_id UInt32", "lead_hour UInt16"]
        columns += [f"{identifier(v)} Array(Float32) CODEC(ZSTD(1))" for v in fields]
        columns += [f"{identifier(v)} Float32 CODEC(ZSTD(1))" for v in ground_fields]
        # No DROP/IF NOT EXISTS: a unique candidate is either new or rejected.
        c.command(
            f"CREATE TABLE {DATABASE}.{table} ({','.join(columns)}) ENGINE=MergeTree PARTITION BY (revision_id,intDiv(cell_id,32768)) ORDER BY (cell_id,lead_hour) SETTINGS index_granularity=256,index_granularity_bytes=0,old_parts_lifetime=60"
        )
        batch = 1024 if meta["model"] == "gfs" else 4096
        for start in range(0, len(ids), batch):
            disk_guard(root)
            gather_started = time.perf_counter()
            end = min(len(ids), start + batch)
            rows = (end - start) * len(leads)
            # Fill Arrow's final contiguous column buffers directly. Stacking
            # cell/lead/level/field first and then copying every field doubles
            # the large intermediate allocation and memory traffic.
            buffers = [
                np.empty((end - start, len(leads), levels), dtype=np.float32)
                for _ in fields
            ]
            ground_buffers = [
                np.empty((end - start, len(leads)), dtype=np.float32)
                for _ in ground_fields
            ]
            for lead_index, (profile, surface) in enumerate(
                zip(profiles, surfaces, strict=True)
            ):
                block = profile[start:end]
                ground = surface[start:end]
                if not np.isfinite(block).all() or not np.isfinite(ground).all():
                    raise ValueError("Non-finite staged values")
                for field_index, buffer in enumerate(buffers):
                    buffer[:, lead_index, :] = block[:, :, field_index]
                for field_index, buffer in enumerate(ground_buffers):
                    buffer[:, lead_index] = ground[:, field_index]
            data = {
                "revision_id": pa.array(np.ones(rows, dtype=np.uint32)),
                "cell_id": pa.array(np.repeat(ids[start:end], len(leads))),
                "lead_hour": pa.array(
                    np.tile(np.asarray(leads, dtype=np.uint16), end - start)
                ),
            }
            for name, buffer in zip(fields, buffers, strict=True):
                data[name] = pa.FixedSizeListArray.from_arrays(
                    pa.array(buffer.reshape(-1)),
                    levels,
                )
            for name, buffer in zip(ground_fields, ground_buffers, strict=True):
                data[name] = pa.array(buffer.reshape(-1))
            timings["gather_arrow_seconds"] += time.perf_counter() - gather_started
            insert_started = time.perf_counter()
            c.insert_arrow(
                f"{DATABASE}.{table}", pa.table(data), settings={"max_threads": 2}
            )
            timings["insert_seconds"] += time.perf_counter() - insert_started
            del data, buffers, ground_buffers, buffer
            if start % (batch * 10) == 0:
                print(
                    json.dumps(
                        {
                            "phase": "load",
                            "model": meta["model"],
                            "cells": end,
                            "total": len(ids),
                        }
                    ),
                    flush=True,
                )
        validation_started = time.perf_counter()
        result = validate(path, table, DATABASE, staged=True)
        timings.update(
            validation_seconds=time.perf_counter() - validation_started,
            total_seconds=time.perf_counter() - started,
            process_read_bytes=process.io_counters().read_bytes - io_before.read_bytes,
            process_write_bytes=process.io_counters().write_bytes
            - io_before.write_bytes,
            io_scope="Updater process counters; may include reaped prefetched extractor children",
        )
        atomic_json(path / "load-timings.json", timings)
        return result
    finally:
        c.close()


def validate(path, table, database=DATABASE, staged=False):
    (path / "validated.json").unlink(missing_ok=True)
    identifier(table)
    identifier(database)
    meta = json.loads((path / "metadata.json").read_text())
    ids = np.load(path / "cells.npz")["cell_id"]
    if meta["coverage"] != "conus" or meta["leads"] != LEADS[meta["model"]]:
        raise ValueError("Incorrect regional coverage or leads")
    if len(ids) != meta["cells"] or not np.all(np.diff(ids.astype(np.int64)) > 0):
        raise ValueError("Incorrect cell index")
    c = client()
    try:
        evidence = validate_keys(c, table, ids, len(meta["leads"]), database=database)
        observed = c.query(
            f"SELECT lead_hour,count() FROM {database}.{table} GROUP BY lead_hour ORDER BY lead_hour"
        ).result_rows
        if list(observed) != [(lead, len(ids)) for lead in meta["leads"]]:
            raise ValueError("Incorrect per-lead counts")
        checks = [
            f"length({identifier(v)}) != {len(meta['levels'])} OR arrayExists(x -> NOT isFinite(x),{v})"
            for v in meta["variables"]
        ]
        checks += [f"NOT isFinite({identifier(v)})" for v in meta["surface_variables"]]
        if c.query(
            f"SELECT count() FROM {database}.{table} WHERE revision_id != 1 OR {' OR '.join(checks)}"
        ).first_row[0]:
            raise ValueError("Malformed/non-finite database values")
        samples = np.unique(
            np.concatenate(
                ([0, len(ids) - 1], np.random.default_rng(17).integers(0, len(ids), 8))
            )
        )
        for index in samples:
            result, _ = query_profile(
                c,
                table,
                meta["variables"] + meta["surface_variables"],
                ids[index],
                database=database,
            )
            if [row[0] for row in result.result_rows] != meta["leads"]:
                raise ValueError("Incomplete sampled profile")
            if staged:
                for lead, row in zip(meta["leads"], result.result_rows, strict=True):
                    source = np.load(path / f"lead-{lead:03d}.npy", mmap_mode="r")
                    ground = np.load(path / f"surface-{lead:03d}.npy", mmap_mode="r")
                    np.testing.assert_array_equal(
                        np.asarray(
                            row[1 : 1 + len(meta["variables"])], dtype=np.float32
                        ).T,
                        source[index],
                    )
                    np.testing.assert_array_equal(
                        np.asarray(row[1 + len(meta["variables"]) :], dtype=np.float32),
                        ground[index],
                    )
        evidence.update(
            table=table,
            database=database,
            init_time=meta["init_time"],
            snapshot_id=meta["snapshot_id"],
            samples=len(samples),
            staged_comparison=staged,
            validated_at=time.time(),
        )
        atomic_json(path / "validated.json", evidence)
        return evidence
    finally:
        c.close()


def clean_workspace(path):
    # Only regenerable arrays/cache in the managed revision; preserve metadata,
    # grid coordinates, extraction evidence and logs for serving/debugging.
    for pattern in ("lead-*.npy", "surface-*.npy"):
        for file in path.glob(pattern):
            file.unlink()
    shutil.rmtree(path / "source-cache", ignore_errors=True)


def collect(root, grace=600):
    """Caller must own update.lock; readers are drained before deleting tables."""
    c = client()
    try:
        with lock(root, exclusive=True):
            state = catalog(root)
            keep = {
                e["revision"]
                for section in ("current", "previous")
                for e in state[section].values()
            }
            pending = []
            for entry in state["retired"]:
                if (
                    entry["revision"] in keep
                    or time.time() - entry["retired_at"] < grace
                ):
                    pending.append(entry)
                    continue
                if entry.get("owned", False):
                    c.command(
                        f"DROP TABLE IF EXISTS {identifier(entry['database'])}.{identifier(entry['table'])} SYNC"
                    )
                shutil.rmtree(descriptor_path(root, entry), ignore_errors=True)
            state["retired"] = pending
            atomic_json(Path(root) / "catalog.json", state)
    finally:
        c.close()
