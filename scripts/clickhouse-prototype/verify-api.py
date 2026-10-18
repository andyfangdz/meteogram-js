"""Verify real full-horizon HTTP profiles against staged Float32 arrays."""

import argparse
import gzip
import importlib.util
import json
import subprocess
import sys
import time
import urllib.error
import urllib.parse
import urllib.request
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

import numpy as np
from benchmark import client, distribution, query_profile, table_stats
from pyproj import CRS, Transformer


def request(model, lat, lon):
    url = "http://127.0.0.1:18080/forecast?" + urllib.parse.urlencode(
        {"model": model, "lat": lat, "lon": lon}
    )
    started = time.perf_counter()
    req = urllib.request.Request(url, headers={"Accept-Encoding": "gzip"})
    try:
        response = urllib.request.urlopen(req, timeout=30)
    except urllib.error.HTTPError as error:
        response = error
    with response:
        raw = response.read()
        decoded = (
            gzip.decompress(raw)
            if response.headers.get("Content-Encoding") == "gzip"
            else raw
        )
        body = json.loads(decoded)
        return body, {
            "status": response.status,
            "ms": (time.perf_counter() - started) * 1000,
            "gzip_bytes": len(raw),
            "json_bytes": len(decoded),
        }


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--data-root", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    script = Path(__file__).with_name("serve.py")
    spec = importlib.util.spec_from_file_location("prototype_serve", script)
    serve = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(serve)
    assert serve.clean([float("nan"), float("inf"), -float("inf"), 1.0]) == [
        None,
        None,
        None,
        1.0,
    ]
    log = args.output.with_suffix(".server.log").open("w")
    server = subprocess.Popen(
        [sys.executable, str(script), "--data-root", str(args.data_root)],
        stdout=log,
        stderr=subprocess.STDOUT,
    )
    query_tag = f"resident_final_{time.time_ns()}"
    result = {
        "query_tag": query_tag,
        "null_conversion_unit_check": True,
        "models": {},
        "scope": "Loopback, native profiles, one resident cycle; no WAN or atomic update test.",
    }
    try:
        for _ in range(60):
            try:
                request("invalid", 0, 0)
                break
            except OSError:
                time.sleep(0.5)
        else:
            raise RuntimeError("API readiness timeout")
        for model in ("hrrr", "gfs", "aifs"):
            data = args.data_root / model
            meta = json.loads((data / "metadata.json").read_text())
            with np.load(data / "cells.npz") as z:
                cells = {k: z[k] for k in z.files}
            profiles = [
                np.load(data / f"lead-{lead:03d}.npy", mmap_mode="r")
                for lead in meta["leads"]
            ]
            surfaces = [
                np.load(data / f"surface-{lead:03d}.npy", mmap_mode="r")
                for lead in meta["leads"]
            ]
            points = [
                ("atlanta", 33.6407, -84.4277),
                ("seattle", 47.61, -122.33),
                ("miami", 25.76, -80.19),
                ("los_angeles", 34.05, -118.24),
                ("denver", 39.74, -104.99),
                ("boston", 42.36, -71.06),
            ]
            indices = sorted(
                {
                    0,
                    len(cells["cell_id"]) - 1,
                    *[int(np.argmin(cells[k])) for k in ("latitude", "longitude")],
                    *[int(np.argmax(cells[k])) for k in ("latitude", "longitude")],
                    *np.random.default_rng(20261017).choice(
                        len(cells["cell_id"]), 8, replace=False
                    ),
                }
            )
            points += [
                (
                    f"cell-{cells['cell_id'][i]}",
                    float(cells["latitude"][i]),
                    float(cells["longitude"][i]),
                )
                for i in indices
            ]
            checks = []
            for name, lat, lon in points:
                body, stats = request(model, lat, lon)
                assert stats["status"] == 200, (model, name, body)
                grid = meta["grid"]
                if grid["kind"] == "projected":
                    px, py = Transformer.from_crs(
                        "EPSG:4326", CRS.from_json_dict(grid["crs"]), always_xy=True
                    ).transform(lon, lat)
                    expected_cell = int(
                        np.argmin(np.abs(np.asarray(grid["y"]) - py))
                    ) * len(grid["x"]) + int(
                        np.argmin(np.abs(np.asarray(grid["x"]) - px))
                    )
                else:
                    expected_cell = int(
                        np.argmin(np.abs(np.asarray(grid["latitude"]) - lat))
                    ) * len(grid["longitude"]) + int(
                        np.argmin(
                            np.abs(
                                (np.asarray(grid["longitude"]) - lon + 180) % 360 - 180
                            )
                        )
                    )
                assert body["grid"]["cell_id"] == expected_cell
                if name.startswith("cell-"):
                    assert expected_cell == int(name.removeprefix("cell-"))
                assert (
                    body["snapshot_id"] == meta["snapshot_id"]
                    and body["init_time"] == meta["init_time"]
                )
                assert body["levels_hpa"] == meta["levels"]
                assert (
                    body["fields"]
                    == ["lead_hour"] + meta["variables"] + meta["surface_variables"]
                )
                assert [r[0] for r in body["rows"]] == meta["leads"]
                i = int(np.searchsorted(cells["cell_id"], body["grid"]["cell_id"]))
                assert int(cells["cell_id"][i]) == body["grid"]["cell_id"]
                assert body["grid"]["latitude"] == float(cells["latitude"][i]) and body[
                    "grid"
                ]["longitude"] == float(cells["longitude"][i])
                for t, row in enumerate(body["rows"]):
                    for v in range(len(meta["variables"])):
                        assert len(row[v + 1]) == len(meta["levels"])
                        np.testing.assert_array_equal(
                            np.asarray(row[v + 1], dtype=np.float32),
                            profiles[t][i, :, v],
                        )
                    np.testing.assert_array_equal(
                        np.asarray(row[1 + len(meta["variables"]) :], dtype=np.float32),
                        surfaces[t][i],
                    )
                checks.append(
                    {
                        "name": name,
                        "requested": [lat, lon],
                        "selected": body["grid"],
                        **stats,
                        "all_fields_times_levels_equal": True,
                    }
                )
            invalid = []
            for lat, lon, expected in [
                (float("nan"), -90, 400),
                (float("inf"), -90, 400),
                (91, 0, 400),
                (40, 181, 400),
                (0, 0, 404),
                (60, -150, 404),
                (23, -100, 404),
                (24 - 1e-8, -100, 404),
                (40, -125 - 1e-8, 404),
                (40, -126, 404),
            ]:
                body, stats = request(model, lat, lon)
                assert stats["status"] == expected, (model, lat, lon, body)
                invalid.append({"latitude": str(lat), "longitude": str(lon), **stats})

            def timed(i, points=points, model=model):
                _, lat, lon = points[i % len(points)]
                _, stats = request(model, lat, lon)
                return stats

            latency = {}
            for concurrency, count in [(1, 100), (4, 300)]:
                before = time.perf_counter()
                with ThreadPoolExecutor(max_workers=concurrency) as pool:
                    samples = list(pool.map(timed, range(count)))
                elapsed = time.perf_counter() - before
                errors = [s for s in samples if s["status"] != 200]
                latency[str(concurrency)] = {
                    "latency_ms": distribution([s["ms"] for s in samples]),
                    "requests_per_second": count / elapsed,
                    "errors": errors,
                    "gzip_bytes": distribution([s["gzip_bytes"] for s in samples]),
                }
                assert not errors, errors
            result["models"][model] = {
                "checks": checks,
                "invalid": invalid,
                "clients": latency,
            }
            args.output.write_text(json.dumps(result, indent=2))
            print(
                json.dumps(
                    {"model": model, "verified_points": len(checks), "latency": latency}
                ),
                flush=True,
            )
        conn = client()
        try:
            result["final_resident_storage"] = {
                model: table_stats(conn, f"{model}_256_zstd")
                for model in ("hrrr", "gfs", "aifs")
            }
            result["resident_database_queries"] = {}
            for model in ("hrrr", "gfs", "aifs"):
                meta = json.loads(
                    (args.data_root / model / "metadata.json").read_text()
                )
                with np.load(args.data_root / model / "cells.npz") as z:
                    ids = z["cell_id"]
                ids = np.random.default_rng(20261017).choice(ids, 300, replace=False)
                fields = meta["variables"] + meta["surface_variables"]
                table = f"{model}_256_zstd"
                level_count = len(meta["levels"])
                coverage_started = time.perf_counter()
                lengths = " OR ".join(
                    f"length({field}) != {level_count}" for field in meta["variables"]
                )
                coverage = conn.query(
                    f"SELECT uniqExact(cell_id), countIf({lengths}) FROM meteogram_bench.{table}"
                ).first_row
                per_lead = conn.query(
                    f"SELECT lead_hour,count() FROM meteogram_bench.{table} GROUP BY lead_hour ORDER BY lead_hour"
                ).result_rows
                assert tuple(coverage) == (meta["cells"], 0), coverage
                assert per_lead == [(lead, meta["cells"]) for lead in meta["leads"]], (
                    per_lead
                )
                result["models"][model]["table_wide_coverage"] = {
                    "distinct_cells": coverage[0],
                    "bad_pressure_array_lengths": coverage[1],
                    "per_lead_rows": per_lead,
                    "seconds": time.perf_counter() - coverage_started,
                }
                for cell in ids:
                    query_profile(conn, table, fields, cell)
                measurements = {"warmup_queries": len(ids)}
                for mode in ("warm", "direct_io"):
                    if mode == "direct_io":
                        conn.command("SYSTEM DROP MARK CACHE")
                    times = []
                    for cell in ids[:30] if mode == "direct_io" else ids:
                        response, elapsed = query_profile(
                            conn,
                            table,
                            fields,
                            cell,
                            direct=mode == "direct_io",
                            tag=f"{query_tag}_{model}_{mode}",
                        )
                        assert [r[0] for r in response.result_rows] == meta["leads"]
                        times.append(elapsed)
                    measurements[mode] = distribution(times)

                def parallel_read(group, table=table, fields=fields):
                    c = client()
                    try:
                        return [
                            query_profile(c, table, fields, cell)[1] for cell in group
                        ]
                    finally:
                        c.close()

                start = time.perf_counter()
                with ThreadPoolExecutor(max_workers=4) as pool:
                    times = [
                        elapsed
                        for group in pool.map(parallel_read, np.array_split(ids, 4))
                        for elapsed in group
                    ]
                measurements["four_clients"] = {
                    "latency_ms": distribution(times),
                    "requests_per_second": len(times) / (time.perf_counter() - start),
                }
                result["resident_database_queries"][model] = measurements
            conn.command("SYSTEM FLUSH LOGS")
            result["resident_query_log"] = conn.query(
                "SELECT log_comment,count(),sum(ProfileEvents['OSReadBytes']),max(memory_usage) "
                "FROM system.query_log WHERE type='QueryFinish' AND startsWith(log_comment,{tag:String}) "
                "GROUP BY log_comment ORDER BY log_comment",
                parameters={"tag": query_tag},
            ).result_rows
            result["resident_query_log_columns"] = [
                "label",
                "queries",
                "os_read_bytes",
                "peak_tracked_memory",
            ]
            result["all_parts"] = conn.query(
                "SELECT table,active,count(),sum(bytes_on_disk) FROM system.parts "
                "WHERE database='meteogram_bench' GROUP BY table,active ORDER BY table,active"
            ).result_rows
            result["all_parts_columns"] = ["table", "active", "parts", "bytes_on_disk"]
        finally:
            conn.close()
        args.output.write_text(json.dumps(result, indent=2))
    finally:
        server.terminate()
        server.wait(timeout=15)
        log.close()


if __name__ == "__main__":
    main()
