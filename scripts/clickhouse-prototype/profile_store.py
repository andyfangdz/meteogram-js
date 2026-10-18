"""One row per cell, flat Float32 pressure/forecast arrays; versioned physical layout."""

import json
import time
from concurrent.futures import ThreadPoolExecutor

import numpy as np
import pyarrow as pa
from clickhouse_connect.driver.query import QueryResult
from profile_pack import backend, library, native_block, pack

LAYOUT = "cell_profile_v1"
CODECS = {
    "zstd": "ZSTD(1)",
    "gorilla": "Gorilla,ZSTD(1)",
    "lz4": "LZ4",
    "delta": "Delta(4),ZSTD(1)",
    "t64": "T64,ZSTD(1)",
}


def read_profile(c, table, fields, cell, meta, database, settings):
    """Return the legacy lead/pressure API shape without SQL array replication."""
    from native_store import identifier

    result = c.query(
        f"SELECT {','.join(identifier(v) for v in fields)} FROM {identifier(database)}.{identifier(table)} WHERE revision_id=1 AND cell_id={{cell:UInt32}}",
        parameters={"cell": int(cell)},
        settings=settings,
    )
    if len(result.result_rows) != 1:
        if not result.result_rows:
            return QueryResult([], column_names=("lead_hour", *fields))
        raise ValueError("Duplicate profile rows")
    row = result.result_rows[0]
    encoded = meta["storage"].get("encoding") == "float32_bits"
    T = len(meta["leads"])
    L = len(meta["levels"])
    arrays = []
    for name, values in zip(fields, row, strict=True):
        if encoded:
            values = np.asarray(values, dtype=np.uint32).view(np.float32)
        if name in meta["variables"]:
            if len(values) != T * L:
                raise ValueError("Malformed pressure profile")
            values = np.asarray(values, dtype=np.float32)
            values = (
                values.reshape(L, T).T
                if meta["storage"]["axes"] == "level_lead"
                else values.reshape(T, L)
            )
            arrays.append(values.tolist())
        else:
            if len(values) != T:
                raise ValueError("Malformed surface profile")
            arrays.append(values.tolist() if encoded else values)
    return QueryResult(
        [
            (lead, *(values[t] for values in arrays))
            for t, lead in enumerate(meta["leads"])
        ],
        column_names=("lead_hour", *fields),
    )


def load(
    root,
    path,
    table,
    *,
    codec="t64",
    axes="level_lead",
    mode="auto",
    granularity=4,
    transport="auto",
):
    """Load immutable candidates; caller runs full storage-specific validation."""
    from benchmark import client
    from native_store import DATABASE, atomic_json, disk_guard, identifier

    if (
        codec not in CODECS
        or axes not in ("level_lead", "lead_level")
        or granularity not in (1, 4, 8, 16)
    ):
        raise ValueError("Unsupported profile storage settings")
    meta = json.loads((path / "metadata.json").read_text())
    ids = np.load(path / "cells.npz")["cell_id"]
    profiles = []
    surfaces = []
    for lead in meta["leads"]:
        report = json.loads((path / f"lead-{lead:03d}.json").read_text())
        if not report["fields"] or any(
            v["finite"] != v["total"] for v in report["fields"].values()
        ):
            raise ValueError("Non-finite source")
        p = np.load(path / f"lead-{lead:03d}.npy", mmap_mode="r")
        s = np.load(path / f"surface-{lead:03d}.npy", mmap_mode="r")
        if p.shape != (
            len(ids),
            len(meta["levels"]),
            len(meta["variables"]),
        ) or s.shape != (len(ids), len(meta["surface_variables"])):
            raise ValueError("Malformed staged shape")
        profiles.append(p)
        surfaces.append(s[:, None, :])
    if transport == "auto":
        transport = "native" if library() is not None else "arrow"
    if transport not in ("native", "arrow"):
        raise ValueError("Unsupported transport")
    encoded = codec in ("delta", "t64")
    meta["storage"] = {
        "encoding": "float32_bits" if encoded else "float32",
        "writer": "rust_native" if transport == "native" else "arrow",
        "layout": LAYOUT,
        "axes": axes,
        "codec": codec,
        "index_granularity": granularity,
    }
    atomic_json(path / "metadata.json", meta)
    c = client()
    pool = ThreadPoolExecutor(max_workers=1, thread_name_prefix="profile-pack")
    started = time.perf_counter()
    timings = {
        "pack_seconds": 0.0,
        "insert_seconds": 0.0,
        "encode_seconds": 0.0,
        "pack_wait_seconds": 0.0,
        "packing_pipeline": True,
        "kernel": backend() if mode == "auto" else mode,
    }
    try:
        fields = meta["variables"] + meta["surface_variables"]
        cols = ["revision_id UInt32", "cell_id UInt32"] + [
            f"{identifier(v)} Array({'UInt32' if encoded else 'Float32'}) CODEC({CODECS[codec]})"
            for v in fields
        ]
        c.command(f"CREATE DATABASE IF NOT EXISTS {DATABASE}")
        c.command(
            f"CREATE TABLE {DATABASE}.{identifier(table)} ({','.join(cols)}) ENGINE=MergeTree PARTITION BY (revision_id,intDiv(cell_id,32768)) ORDER BY cell_id SETTINGS index_granularity={granularity},index_granularity_bytes=0,old_parts_lifetime=60"
        )
        batch = 2048 if meta["model"] == "gfs" else 4096

        def pack_batch(start):
            begin = time.perf_counter()
            end = min(start + batch, len(ids))
            packed = pack([p[start:end] for p in profiles], mode)
            if axes == "lead_level":
                packed = np.ascontiguousarray(packed.transpose(0, 1, 3, 2))
            ground = pack([s[start:end] for s in surfaces], mode)
            return packed, ground, time.perf_counter() - begin

        pending = pool.submit(pack_batch, 0) if len(ids) else None
        for start in range(0, len(ids), batch):
            disk_guard(root)
            end = min(start + batch, len(ids))
            n = end - start
            begin = time.perf_counter()
            packed, ground, pack_seconds = pending.result()
            timings["pack_wait_seconds"] += time.perf_counter() - begin
            timings["pack_seconds"] += pack_seconds
            pending = pool.submit(pack_batch, end) if end < len(ids) else None
            begin = time.perf_counter()
            if transport == "native":
                with native_block(
                    ids[start:end], fields, [*packed, *ground], encoded
                ) as block:
                    timings["encode_seconds"] += time.perf_counter() - begin
                    begin = time.perf_counter()
                    c.raw_insert(
                        f"{DATABASE}.{table}",
                        insert_block=block,
                        fmt="Native",
                        settings={"max_threads": 2},
                    )
                    timings["insert_seconds"] += time.perf_counter() - begin
            else:
                data = {
                    "revision_id": pa.array(np.ones(n, dtype=np.uint32)),
                    "cell_id": pa.array(ids[start:end]),
                }
                for f, name in enumerate(meta["variables"]):
                    data[name] = pa.FixedSizeListArray.from_arrays(
                        pa.array(
                            packed[f].reshape(-1).view(np.uint32)
                            if encoded
                            else packed[f].reshape(-1)
                        ),
                        len(meta["levels"]) * len(meta["leads"]),
                    )
                for f, name in enumerate(meta["surface_variables"]):
                    data[name] = pa.FixedSizeListArray.from_arrays(
                        pa.array(
                            ground[f].reshape(-1).view(np.uint32)
                            if encoded
                            else ground[f].reshape(-1)
                        ),
                        len(meta["leads"]),
                    )
                timings["encode_seconds"] += time.perf_counter() - begin
                begin = time.perf_counter()
                c.insert_arrow(
                    f"{DATABASE}.{table}", pa.table(data), settings={"max_threads": 2}
                )
                timings["insert_seconds"] += time.perf_counter() - begin
                del data
            del packed, ground
            if start % (batch * 10) == 0:
                print(
                    json.dumps(
                        {
                            "phase": "profile_load",
                            "model": meta["model"],
                            "cells": end,
                            "total": len(ids),
                        }
                    ),
                    flush=True,
                )
        timings["total_insert_seconds"] = time.perf_counter() - started
        atomic_json(path / "load-timings.json", timings)
        return timings
    finally:
        pool.shutdown(wait=True, cancel_futures=True)
        c.close()
