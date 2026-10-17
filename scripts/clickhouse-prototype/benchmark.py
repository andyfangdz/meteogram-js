"""Load measured real profiles into isolated ClickHouse benchmark tables.

Never use a production database: this creates/replaces named tables inside the
dedicated meteogram_bench database. Fetching source data is a separate step.
"""

import argparse
import gzip
import json
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import clickhouse_connect
import numpy as np
import psutil
import pyarrow as pa

DATABASE = "meteogram_bench"


def client():
    return clickhouse_connect.get_client(
        host="127.0.0.1", port=18123, autogenerate_session_id=False
    )


def distribution(values):
    return {
        "count": len(values),
        "min": float(np.min(values)),
        "p50": float(np.percentile(values, 50)),
        "p95": float(np.percentile(values, 95)),
        "p99": float(np.percentile(values, 99)),
        "max": float(np.max(values)),
    }


def query_profile(c, table, fields, cell, revision=1, direct=False, tag=""):
    started = time.perf_counter()
    result = c.query(
        f"SELECT lead_hour,{','.join(fields)} FROM {DATABASE}.{table} WHERE revision_id={{revision:UInt32}} AND cell_id={{cell:UInt32}} ORDER BY lead_hour",
        parameters={"revision": revision, "cell": int(cell)},
        settings={
            "max_threads": 2,
            "use_query_cache": 0,
            "use_uncompressed_cache": 0,
            "min_bytes_to_use_direct_io": 1 if direct else 0,
            "log_comment": tag,
        },
    )
    return result, (time.perf_counter() - started) * 1000


def table_stats(c, table):
    rows = c.query(
        f"SELECT sum(rows),sum(bytes_on_disk),sum(data_compressed_bytes),sum(data_uncompressed_bytes),sum(marks_bytes),count() FROM system.parts WHERE database='{DATABASE}' AND table='{table}' AND active"
    ).first_row
    return dict(
        zip(
            [
                "rows",
                "bytes_on_disk",
                "data_compressed_bytes",
                "data_uncompressed_bytes",
                "marks_bytes",
                "active_parts",
            ],
            map(int, rows),
        )
    )


def validate_keys(c, table, ids, lead_count, cell_span=32768):
    """Check total and per-range unique-key counts with bounded memory.

    A single uniqExact over ~80 million HRRR keys can exceed the configured
    2 GiB query limit. Disjoint cell-ID ranges bound each aggregation instead.
    """
    expected_total = len(ids) * lead_count
    total = int(c.query(f"SELECT count() FROM {DATABASE}.{table}").first_row[0])
    if total != expected_total:
        raise ValueError(f"Incorrect row count: {total}; expected {expected_total}")
    started = time.perf_counter()
    ranges = 0
    for lower in range(int(ids[0]), int(ids[-1]) + 1, cell_span):
        upper = lower + cell_span
        expected = (
            int(np.searchsorted(ids, upper) - np.searchsorted(ids, lower)) * lead_count
        )
        actual = c.query(
            f"SELECT count(),uniqExact(tuple(cell_id,lead_hour)) FROM {DATABASE}.{table} "
            "WHERE cell_id >= {lower:UInt32} AND cell_id < {upper:UInt32}",
            parameters={"lower": lower, "upper": upper},
        ).first_row
        if tuple(actual) != (expected, expected):
            raise ValueError(
                f"Missing or duplicate keys in [{lower}, {upper}): {actual}; expected {expected}"
            )
        ranges += 1
    return {
        "rows": total,
        "cell_id_span": cell_span,
        "ranges": ranges,
        "seconds": time.perf_counter() - started,
    }


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--layouts", default="8192_lz4,256_lz4,256_zstd,512_zstd,256_gorilla"
    )
    parser.add_argument("--batch-cells", type=int, default=1024)
    parser.add_argument("--queries", type=int, default=120)
    parser.add_argument("--server-pid", type=int, required=True)
    parser.add_argument("--read-during-ingest", action="store_true")
    args = parser.parse_args()
    metadata = json.loads((args.data / "metadata.json").read_text())
    leads = metadata["leads"]
    for lead in leads:
        if not (args.data / f"lead-{lead:03d}.json").exists():
            raise ValueError(f"Incomplete source extraction: lead {lead}")
    ids = np.load(args.data / "cells.npz")["cell_id"]
    profiles = [np.load(args.data / f"lead-{l:03d}.npy", mmap_mode="r") for l in leads]
    surfaces = [
        np.load(args.data / f"surface-{l:03d}.npy", mmap_mode="r") for l in leads
    ]
    fields = metadata["variables"] + metadata["surface_variables"]
    nlevels = len(metadata["levels"])
    server = psutil.Process(args.server_pid)
    c = client()
    c.command(f"CREATE DATABASE IF NOT EXISTS {DATABASE}")
    run_id = f"{metadata['model']}_{time.time_ns()}"
    result = {
        "run_id": run_id,
        "model": metadata["model"],
        "init_time": metadata["init_time"],
        "cells": len(ids),
        "leads": leads,
        "levels": metadata["levels"],
        "clickhouse_version": c.server_version,
        "batch_cells": args.batch_cells,
        "layouts": {},
    }
    rng = np.random.default_rng(20261017)
    query_indices = rng.choice(
        len(ids), size=min(args.queries, len(ids)), replace=False
    )
    first_table = None
    for layout in args.layouts.split(","):
        granularity, compression = layout.split("_")
        if granularity not in ("256", "512", "8192") or compression not in (
            "lz4",
            "zstd",
            "gorilla",
        ):
            raise ValueError("Unsupported benchmark layout")
        table = f"{metadata['model']}_{layout}"
        tag = f"{run_id}_{layout}"
        codec = {"zstd": "ZSTD(1)", "lz4": "LZ4", "gorilla": "Gorilla,ZSTD(1)"}[
            compression
        ]
        columns = (
            ["revision_id UInt32", "cell_id UInt32", "lead_hour UInt16"]
            + [f"{v} Array(Float32) CODEC({codec})" for v in metadata["variables"]]
            + [f"{v} Float32 CODEC({codec})" for v in metadata["surface_variables"]]
        )
        c.command(f"DROP TABLE IF EXISTS {DATABASE}.{table} SYNC")
        c.command(
            f"CREATE TABLE {DATABASE}.{table} ({','.join(columns)}) ENGINE=MergeTree PARTITION BY revision_id ORDER BY (cell_id,lead_hour) SETTINGS index_granularity={granularity},index_granularity_bytes=0"
        )
        stop = threading.Event()
        concurrent_latencies = []
        concurrent_errors = []

        def during_insert(
            stop=stop,
            first_table=first_table,
            tag=tag,
            concurrent_latencies=concurrent_latencies,
            concurrent_errors=concurrent_errors,
            cells=None,
        ):
            reader = client()
            n = 0
            while not stop.is_set():
                try:
                    _, elapsed = query_profile(
                        reader,
                        first_table,
                        fields,
                        (
                            cells[n % len(cells)]
                            if cells is not None
                            else ids[query_indices[n % len(query_indices)]]
                        ),
                        tag=f"{tag}_during_insert",
                    )
                    concurrent_latencies.append(elapsed)
                except Exception as error:  # noqa: BLE001 -- retain all reader failures in benchmark evidence
                    concurrent_errors.append(str(error))
                n += 1
                stop.wait(0.05)
            reader.close()

        reader_thread = None
        if first_table:
            reader_thread = threading.Thread(target=during_insert, daemon=True)
            reader_thread.start()
        started = time.perf_counter()
        server_cpu_before = sum(server.cpu_times()[:2])
        server_io_before = server.io_counters().write_bytes
        client_cpu_before = time.process_time()
        max_server_rss = 0
        arrow_bytes = 0
        batches = 0
        for start in range(0, len(ids), args.batch_cells):
            end = min(len(ids), start + args.batch_cells)
            # Bounded transpose: cell, lead, pressure, variable.
            block = np.stack([p[start:end] for p in profiles], axis=1)
            ground = np.stack([p[start:end] for p in surfaces], axis=1)
            rows = (end - start) * len(leads)
            data = {
                "revision_id": pa.array(np.full(rows, 1, dtype=np.uint32)),
                "cell_id": pa.array(np.repeat(ids[start:end], len(leads))),
                "lead_hour": pa.array(
                    np.tile(np.asarray(leads, dtype=np.uint16), end - start)
                ),
            }
            for variable_index, name in enumerate(metadata["variables"]):
                values = np.ascontiguousarray(block[:, :, :, variable_index]).reshape(
                    -1
                )
                data[name] = pa.FixedSizeListArray.from_arrays(
                    pa.array(values), nlevels
                )
            for variable_index, name in enumerate(metadata["surface_variables"]):
                data[name] = pa.array(
                    np.ascontiguousarray(ground[:, :, variable_index]).reshape(-1)
                )
            arrow = pa.table(data)
            arrow_bytes += arrow.nbytes
            c.insert_arrow(
                f"{DATABASE}.{table}",
                arrow,
                settings={"max_threads": 2, "log_comment": f"{tag}_insert"},
            )
            batches += 1
            if batches == 1 and not first_table and args.read_during_ingest:
                reader_thread = threading.Thread(
                    target=during_insert,
                    daemon=True,
                    kwargs={
                        "first_table": table,
                        "cells": ids[: end : max(1, end // 120)],
                    },
                )
                reader_thread.start()
            max_server_rss = max(max_server_rss, server.memory_info().rss)
            if batches % 10 == 0:
                print(
                    json.dumps(
                        {
                            "phase": "insert",
                            "model": metadata["model"],
                            "cells": end,
                            "total_cells": len(ids),
                            "elapsed_seconds": time.perf_counter() - started,
                        }
                    ),
                    flush=True,
                )
        ingestion_seconds = time.perf_counter() - started
        stop.set()
        if reader_thread:
            reader_thread.join()
        loaded = {
            "seconds": ingestion_seconds,
            "rows_per_second": len(ids) * len(leads) / ingestion_seconds,
            "arrow_bytes": arrow_bytes,
            "batches": batches,
            "server_cpu_seconds": sum(server.cpu_times()[:2]) - server_cpu_before,
            "client_cpu_seconds": time.process_time() - client_cpu_before,
            "server_write_bytes": server.io_counters().write_bytes - server_io_before,
            "sampled_server_peak_rss_bytes": max_server_rss,
        }
        if concurrent_latencies:
            loaded["queries_during_ingest_ms"] = distribution(concurrent_latencies)
            loaded["queries_during_ingest_errors"] = concurrent_errors
        stats = table_stats(c, table)
        # Wait briefly for ordinary background merges, without forcing OPTIMIZE.
        merge_started = time.perf_counter()
        while time.perf_counter() - merge_started < 60:
            merging = c.query(
                "SELECT count() FROM system.merges WHERE database={db:String} AND table={table:String}",
                parameters={"db": DATABASE, "table": table},
            ).first_row[0]
            if not merging:
                break
            time.sleep(1)
        loaded["merge_wait_seconds"] = time.perf_counter() - merge_started
        key_validation = validate_keys(c, table, ids, len(leads))
        # Scientific round trip: native values survive Arrow + CH exactly as Float32.
        for index in query_indices[:8]:
            response, _ = query_profile(c, table, fields, ids[index])
            assert [r[0] for r in response.result_rows] == leads
            for lead_index, row in enumerate(response.result_rows):
                for variable_index in range(len(metadata["variables"])):
                    np.testing.assert_array_equal(
                        np.asarray(row[variable_index + 1], dtype=np.float32),
                        profiles[lead_index][index, :, variable_index],
                    )
                np.testing.assert_array_equal(
                    np.asarray(row[1 + len(metadata["variables"]) :], dtype=np.float32),
                    surfaces[lead_index][index],
                )
        benchmarks = {}
        for mode in ["warm", "direct_io"]:
            timings = []
            read_rows = []
            read_bytes = []
            count = (
                len(query_indices) if mode == "warm" else min(30, len(query_indices))
            )
            if mode == "direct_io":
                c.command("SYSTEM DROP MARK CACHE")
            for index in query_indices[:count]:
                response, elapsed = query_profile(
                    c,
                    table,
                    fields,
                    ids[index],
                    direct=mode == "direct_io",
                    tag=f"{tag}_{mode}",
                )
                timings.append(elapsed)
                read_rows.append(int(response.summary.get("read_rows", 0)))
                read_bytes.append(int(response.summary.get("read_bytes", 0)))
            benchmarks[mode] = {
                "latency_ms": distribution(timings),
                "read_rows": distribution(read_rows),
                "read_bytes": distribution(read_bytes),
            }

        # Independent client connections: no shared per-query session state.
        def parallel_reader(indices, table=table, tag=tag):
            conn = client()
            elapsed = []
            for index in indices:
                _, duration = query_profile(
                    conn, table, fields, ids[index], tag=f"{tag}_parallel"
                )
                elapsed.append(duration)
            conn.close()
            return elapsed

        started = time.perf_counter()
        with ThreadPoolExecutor(max_workers=4) as pool:
            parallel_times = [
                timing
                for group in pool.map(parallel_reader, np.array_split(query_indices, 4))
                for timing in group
            ]
        benchmarks["four_clients"] = {
            "latency_ms": distribution(parallel_times),
            "requests_per_second": len(parallel_times)
            / (time.perf_counter() - started),
        }
        example, _ = query_profile(c, table, fields, ids[query_indices[0]])
        encoded = json.dumps(
            {
                "levels": metadata["levels"],
                "fields": ["lead_hour"] + fields,
                "rows": example.result_rows,
            },
            separators=(",", ":"),
            allow_nan=False,
        ).encode()
        benchmarks["example_payload"] = {
            "json_bytes": len(encoded),
            "gzip_bytes": len(gzip.compress(encoded, compresslevel=6)),
        }
        c.command("SYSTEM FLUSH LOGS")
        log = c.query(
            f"SELECT log_comment,count(),avg(query_duration_ms),avg(read_rows),avg(read_bytes),max(memory_usage),sum(ProfileEvents['ReadCompressedBytes']),sum(ProfileEvents['OSReadBytes']),sum(ProfileEvents['WriteBufferFromFileDescriptorWriteBytes']) FROM system.query_log WHERE type='QueryFinish' AND startsWith(log_comment,'{tag}_') GROUP BY log_comment ORDER BY log_comment"
        ).result_rows
        result["layouts"][layout] = {
            "ingestion": loaded,
            "storage": stats,
            "queries": benchmarks,
            "query_log_columns": [
                "label",
                "queries",
                "mean_server_ms",
                "mean_read_rows",
                "mean_read_bytes",
                "peak_tracked_memory",
                "compressed_bytes_read",
                "os_read_bytes",
                "file_write_bytes",
            ],
            "query_log": log,
            "roundtrip_verified_cells": min(8, len(query_indices)),
            "key_validation": key_validation,
        }
        args.output.write_text(json.dumps(result, indent=2))
        print(
            json.dumps(
                {
                    "model": metadata["model"],
                    "layout": layout,
                    "ingestion": loaded,
                    "storage": stats,
                    "queries": benchmarks,
                }
            ),
            flush=True,
        )
        if first_table is None:
            first_table = table
    result["final_storage"] = {
        layout: table_stats(c, f"{metadata['model']}_{layout}")
        for layout in result["layouts"]
    }
    args.output.write_text(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
