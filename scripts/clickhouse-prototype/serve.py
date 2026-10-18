"""Local-only arbitrary-coordinate API over the ingested benchmark cells.

Returns native source fields, not the application's derived CloudColumn format.
Out-of-sample locations return 404; they never trigger source downloads.
"""

import argparse
import contextlib
import gzip
import json
import math
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from queue import Queue
from urllib.parse import parse_qs, urlparse

import numpy as np
from benchmark import DATABASE, client, query_profile
from native_store import catalog, descriptor_path, freshness, lock
from pyproj import CRS, Transformer


def resolve(model, lat, lon):
    grid = model["metadata"]["grid"]
    if model["metadata"].get("coverage") == "conus" and not (
        24 <= lat <= 50 and -125 <= lon <= -66
    ):
        raise LookupError("Outside the retained region")
    if grid["kind"] == "projected":
        x, y = model["projection"].transform(lon, lat)
        xs, ys = np.asarray(grid["x"]), np.asarray(grid["y"])
        # A native edge cell can round-trip through lon/lat a few nanometres
        # beyond the projected coordinate bound. Permit numerical noise only.
        tolerance = 1e-6
        if not (
            xs.min() - tolerance <= x <= xs.max() + tolerance
            and ys.min() - tolerance <= y <= ys.max() + tolerance
        ):
            raise LookupError("Outside the native model domain")
        cell = int(np.abs(ys - y).argmin()) * len(xs) + int(np.abs(xs - x).argmin())
    else:
        if not (24 <= lat <= 50 and -125 <= lon <= -66):
            raise LookupError("Outside the retained region")
        lats, lons = np.asarray(grid["latitude"]), np.asarray(grid["longitude"])
        cell = int(np.abs(lats - lat).argmin()) * len(lons) + int(
            np.abs((lons - lon + 180) % 360 - 180).argmin()
        )
    index = int(np.searchsorted(model["cells"]["cell_id"], cell))
    if (
        index >= len(model["cells"]["cell_id"])
        or model["cells"]["cell_id"][index] != cell
    ):
        raise LookupError("Location was not ingested in this bounded prototype")
    return cell, index


def clean(value):
    if isinstance(value, float) and not math.isfinite(value):
        return None
    if isinstance(value, (tuple, list)):
        return [clean(v) for v in value]
    return value


def read_model(path, table, database=DATABASE):
    meta = json.loads((path / "metadata.json").read_text())
    with np.load(path / "cells.npz") as cells:
        model = {
            "metadata": meta,
            "cells": {key: cells[key] for key in cells.files},
            "table": table,
            "database": database,
        }
    if meta["grid"]["kind"] == "projected":
        model["projection"] = Transformer.from_crs(
            "EPSG:4326", CRS.from_json_dict(meta["grid"]["crs"]), always_xy=True
        )
    return model


class Registry:
    def __init__(self, root):
        self.root = root
        self.cache = {}
        self.mutex = threading.Lock()

    @contextlib.contextmanager
    def read(self):
        # Keep the shared lock through the query; GC cannot remove its table.
        with lock(self.root):
            state = catalog(self.root)
            with self.mutex:
                models = {}
                for name, entry in state["current"].items():
                    revision = entry["revision"]
                    if revision not in self.cache:
                        self.cache[revision] = read_model(
                            descriptor_path(self.root, entry),
                            entry["table"],
                            entry["database"],
                        )
                        self.cache[revision]["revision"] = revision
                    models[name] = self.cache[revision]
                self.cache = {model["revision"]: model for model in models.values()}
            yield models


def main():
    p = argparse.ArgumentParser()
    source = p.add_mutually_exclusive_group(required=True)
    source.add_argument("--data-root", type=Path)
    source.add_argument("--catalog-root", type=Path)
    p.add_argument(
        "--layout",
        choices=["256_zstd", "512_zstd", "8192_lz4", "256_lz4"],
        default="256_zstd",
    )
    p.add_argument("--port", type=int, default=18080)
    args = p.parse_args()
    models = {}
    if args.data_root:
        for name in ["hrrr", "gfs", "aifs"]:
            path = args.data_root / name
            if (path / "metadata.json").exists():
                models[name] = read_model(path, f"{name}_{args.layout}")
    registry = Registry(args.catalog_root) if args.catalog_root else None
    pool = Queue(maxsize=4)
    for _ in range(4):
        pool.put(client())

    class Handler(BaseHTTPRequestHandler):
        def do_GET(self):
            parsed = urlparse(self.path)
            if parsed.path == "/health":
                try:
                    with (
                        registry.read()
                        if registry
                        else contextlib.nullcontext(models) as active
                    ):
                        status = {
                            name: freshness(model["metadata"])
                            for name, model in active.items()
                        }
                        conn = pool.get(timeout=5)
                        try:
                            conn.command("SELECT 1")
                        finally:
                            pool.put(conn)
                        healthy = len(status) == 3 and all(
                            item["available"] for item in status.values()
                        )
                        self.respond(
                            200 if healthy else 503,
                            {"models": status, "native_fields": True},
                        )
                except Exception:  # noqa: BLE001 -- HTTP health boundary
                    self.respond(503, {"error": "Native forecast service unavailable"})
                return
            if parsed.path != "/forecast":
                self.respond(404, {"error": "Use /forecast?model=hrrr&lat=41&lon=-74"})
                return
            try:
                params = parse_qs(parsed.query, strict_parsing=True)
                name = params["model"][0]
                lat, lon = float(params["lat"][0]), float(params["lon"][0])
                if name not in ("hrrr", "gfs", "aifs") or not (
                    math.isfinite(lat)
                    and math.isfinite(lon)
                    and -90 <= lat <= 90
                    and -180 <= lon <= 180
                ):
                    raise ValueError("Invalid model or coordinates")
                with (
                    registry.read()
                    if registry
                    else contextlib.nullcontext(models) as active
                ):
                    if name not in active:
                        raise RuntimeError("No published forecast")
                    model = active[name]
                    if registry and not freshness(model["metadata"])["available"]:
                        raise RuntimeError("Published forecast is stale")
                    cell, index = resolve(model, lat, lon)
                    meta = model["metadata"]
                    fields = meta["variables"] + meta["surface_variables"]
                    conn = pool.get(timeout=5)
                    try:
                        result, elapsed = query_profile(
                            conn,
                            model["table"],
                            fields,
                            cell,
                            tag="native_api",
                            database=model["database"],
                        )
                    finally:
                        pool.put(conn)
                    if [row[0] for row in result.result_rows] != meta["leads"]:
                        raise RuntimeError("Incomplete stored profile")
                body = {
                    "model": name,
                    "dataset": meta["dataset"],
                    "init_time": meta["init_time"],
                    "snapshot_id": meta["snapshot_id"],
                    "requested": {"latitude": lat, "longitude": lon},
                    "grid": {
                        "cell_id": cell,
                        "latitude": float(model["cells"]["latitude"][index]),
                        "longitude": float(model["cells"]["longitude"][index]),
                    },
                    "levels_hpa": meta["levels"],
                    "units": meta["units"],
                    "fields": ["lead_hour"] + fields,
                    "rows": clean(result.result_rows),
                    "native_wind_frame": "grid-relative"
                    if name == "hrrr"
                    else "earth-relative",
                    "query_ms": elapsed,
                    "prototype": True,
                    "revision": model.get("revision"),
                }
                self.respond(200, body)
            except (ValueError, KeyError) as error:
                self.respond(400, {"error": str(error)})
            except LookupError as error:
                self.respond(404, {"error": str(error)})
            except Exception:  # noqa: BLE001 -- HTTP boundary returns a bounded availability failure
                self.respond(
                    503, {"error": "Local native forecast unavailable or stale"}
                )

        def respond(self, status, body):
            data = json.dumps(body, separators=(",", ":"), allow_nan=False).encode()
            compressed = "gzip" in self.headers.get("Accept-Encoding", "")
            if compressed:
                data = gzip.compress(data, compresslevel=6)
            self.send_response(status)
            self.send_header("Content-Type", "application/json")
            self.send_header("Content-Length", str(len(data)))
            self.send_header("Cache-Control", "no-store")
            if compressed:
                self.send_header("Content-Encoding", "gzip")
            self.end_headers()
            self.wfile.write(data)

        def log_message(self, format, *args):
            pass

    print(f"Native forecast API listening on 127.0.0.1:{args.port}", flush=True)
    ThreadingHTTPServer(("127.0.0.1", args.port), Handler).serve_forever()


if __name__ == "__main__":
    main()
