"""Fetch real spatial samples once; count GRIB bytes at the codec boundary.

Outputs live outside the repository. HRRR samples four distinct 96x96 tiles;
GFS/AIFS retain every grid cell in the initial US bounding rectangle.
"""

import argparse
import fcntl
import json
import resource
import shutil
import threading
import time
from concurrent.futures import ThreadPoolExecutor
from importlib.util import module_from_spec, spec_from_file_location
from pathlib import Path

import numpy as np
import psutil
import xarray as xr
import zarr
from gribberish.zarr.codec import GribberishCodec
from pyproj import CRS, Transformer
from source_cache import SourceCache

spec = spec_from_file_location(
    "dynamical_probe", Path(__file__).parents[1] / "probe-dynamical.py"
)
probe = module_from_spec(spec)
spec.loader.exec_module(probe)
SURFACE = ["temperature_2m", "dew_point_temperature_2m"]
LOCK = threading.Lock()
COUNTS = {"chunks": 0, "grib_bytes": 0, "decode_seconds": 0.0, "by_variable": {}}
ORIGINAL_DECODE = GribberishCodec._decode_single


async def measured_decode(self, chunk_data, chunk_spec):
    started = time.perf_counter()
    result = await ORIGINAL_DECODE(self, chunk_data, chunk_spec)
    duration = time.perf_counter() - started
    size = len(chunk_data)
    with LOCK:
        COUNTS["chunks"] += 1
        COUNTS["grib_bytes"] += size
        COUNTS["decode_seconds"] += duration
        field = COUNTS["by_variable"].setdefault(
            str(self.var), {"chunks": 0, "bytes": 0}
        )
        field["chunks"] += 1
        field["bytes"] += size
    return result


def snapshot():
    with LOCK:
        return json.loads(json.dumps(COUNTS))


def net_bytes():
    return sum(
        v.bytes_recv
        for k, v in psutil.net_io_counters(pernic=True).items()
        if k != "lo"
    )


def choose_cells(ds, model, tile_size, coverage="sample"):
    if model == "hrrr":
        x, y = ds.x.values, ds.y.values
        crs = CRS.from_cf(ds.spatial_ref.attrs)
        forward = Transformer.from_crs("EPSG:4326", crs, always_xy=True)
        inverse = Transformer.from_crs(crs, "EPSG:4326", always_xy=True)
        cells = []
        tiles = []
        for name, lat, lon in [
            ("california", 37, -120),
            ("rockies", 40, -105),
            ("plains", 37, -97),
            ("northeast", 41, -74),
        ]:
            px, py = forward.transform(lon, lat)
            cx, cy = int(np.abs(x - px).argmin()), int(np.abs(y - py).argmin())
            x0 = max(0, min(len(x) - tile_size, cx - tile_size // 2))
            y0 = max(0, min(len(y) - tile_size, cy - tile_size // 2))
            yy, xx = np.meshgrid(
                np.arange(y0, y0 + tile_size),
                np.arange(x0, x0 + tile_size),
                indexing="ij",
            )
            cells.extend((yy * len(x) + xx).ravel().tolist())
            tiles.append({"name": name, "x0": x0, "y0": y0, "size": tile_size})
        ids = np.unique(cells).astype(np.uint32)
        iy, ix = ids // len(x), ids % len(x)
        lon, lat = inverse.transform(x[ix], y[iy])
        # Count all cells in the proposed geographic rectangle without retaining
        # a full set of 2-D coordinate arrays.
        conus_count = 0
        conus_ids = []
        for start in range(0, len(y), 64):
            xx, yy = np.meshgrid(x, y[start : start + 64])
            llon, llat = inverse.transform(xx, yy)
            mask = (llat >= 24) & (llat <= 50) & (llon >= -125) & (llon <= -66)
            conus_count += int(mask.sum())
            if coverage == "conus":
                conus_ids.append(np.flatnonzero(mask) + start * len(x))
        if coverage == "conus":
            ids = np.concatenate(conus_ids).astype(np.uint32)
            iy, ix = ids // len(x), ids % len(x)
            lon, lat = inverse.transform(x[ix], y[iy])
            tiles = []
        grid = {
            "kind": "projected",
            "crs": crs.to_json_dict(),
            "x": x.tolist(),
            "y": y.tolist(),
            "tiles": tiles,
            "native_cells": len(x) * len(y),
            "conus_cells": conus_count,
        }
        indexers = {
            "y": xr.DataArray(iy.astype(int), dims="cell"),
            "x": xr.DataArray(ix.astype(int), dims="cell"),
        }
    else:
        latitudes, longitudes = ds.latitude.values, ds.longitude.values
        wrapped = (longitudes + 180) % 360 - 180
        yy = np.flatnonzero((latitudes >= 24) & (latitudes <= 50))
        xx = np.flatnonzero((wrapped >= -125) & (wrapped <= -66))
        iy, ix = np.meshgrid(yy, xx, indexing="ij")
        iy, ix = iy.ravel(), ix.ravel()
        ids = (iy * len(longitudes) + ix).astype(np.uint32)
        order = np.argsort(ids)
        ids, iy, ix = ids[order], iy[order], ix[order]
        lat, lon = latitudes[iy], wrapped[ix]
        grid = {
            "kind": "latlon",
            "latitude": latitudes.tolist(),
            "longitude": longitudes.tolist(),
            "native_cells": len(latitudes) * len(longitudes),
            "conus_cells": len(ids),
        }
        indexers = {
            "latitude": xr.DataArray(iy, dims="cell"),
            "longitude": xr.DataArray(ix, dims="cell"),
        }
    return ids, np.asarray(lat), np.asarray(lon), indexers, grid


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--model", choices=probe.MODELS, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--leads", default="all")
    parser.add_argument("--tile-size", type=int, default=96)
    parser.add_argument("--run-offset", type=int, default=2)
    parser.add_argument("--workers", type=int, default=4)
    parser.add_argument("--concurrency", type=int, default=4)
    parser.add_argument("--coverage", choices=["sample", "conus"], default="sample")
    parser.add_argument("--spatial-shards", type=int, default=1)
    parser.add_argument("--shard-index", type=int, default=0)
    parser.add_argument("--cache-dir", type=Path)
    parser.add_argument("--pin-metadata", type=Path)
    parser.add_argument("--offline-cache", action="store_true")
    args = parser.parse_args()
    if not 0 <= args.shard_index < args.spatial_shards:
        parser.error("shard-index must be in [0, spatial-shards)")
    args.output.mkdir(parents=True, exist_ok=True)
    lock_file = (args.output / ".fetch.lock").open("w")
    fcntl.flock(lock_file, fcntl.LOCK_EX | fcntl.LOCK_NB)
    GribberishCodec._decode_single = measured_decode
    zarr.config.set({"async.concurrency": args.concurrency})
    config = probe.MODELS[args.model]
    metadata_path = args.output / "metadata.json"
    previous = json.loads(metadata_path.read_text()) if metadata_path.exists() else None
    pinned = previous or (
        json.loads(args.pin_metadata.read_text()) if args.pin_metadata else None
    )
    if pinned and pinned["dataset"] != config["dataset"]:
        raise ValueError("Pinned snapshot belongs to another dataset")
    if args.offline_cache and (not args.cache_dir or not pinned):
        parser.error("offline-cache requires cache-dir and pinned metadata")
    if previous and previous["dataset"] != config["dataset"]:
        raise ValueError("Existing output belongs to another dataset")
    opening = time.perf_counter()
    for attempt in range(4):
        try:
            if args.offline_cache:
                snapshot_id = pinned["snapshot_id"]
                store = SourceCache.from_cache(
                    args.cache_dir / config["dataset"] / snapshot_id,
                    config["variables"] + SURFACE,
                )
            else:
                store = probe.open_store(
                    config["dataset"],
                    snapshot_id=pinned["snapshot_id"] if pinned else None,
                    catalog_path=args.output / "collection.json",
                )
                session = getattr(store, "session", getattr(store, "_session", None))
                snapshot_id = str(getattr(session, "snapshot_id", "unknown"))
            if args.cache_dir and not args.offline_cache:
                if snapshot_id == "unknown":
                    raise ValueError("Cannot cache a source without a pinned snapshot")
                store = SourceCache(
                    store, args.cache_dir / config["dataset"] / snapshot_id
                )
            ds = xr.open_zarr(
                store, group="pressure_level", chunks=None, consolidated=False
            )
            root = xr.open_zarr(store, chunks=None, consolidated=False)
            break
        except Exception as error:
            if attempt == 3:
                raise
            print(f"Metadata retry {attempt + 1}: {type(error).__name__}", flush=True)
            time.sleep(2**attempt)
    ids, lat, lon, indexers, grid = choose_cells(
        ds, args.model, args.tile_size, args.coverage
    )
    bounds = np.linspace(0, len(ids), args.spatial_shards + 1, dtype=int)
    selection = slice(bounds[args.shard_index], bounds[args.shard_index + 1])
    ids, lat, lon = ids[selection], lat[selection], lon[selection]
    indexers = {key: value.isel(cell=selection) for key, value in indexers.items()}
    levels = np.sort(
        ds.pressure_level.values[
            (ds.pressure_level.values >= 275) & (ds.pressure_level.values <= 1000)
        ]
    )[::-1]
    init = (
        np.datetime64(pinned["init_time"])
        if pinned
        else ds.init_time.values[-args.run_offset]
    )
    leads = (
        ds.lead_time.values.astype("timedelta64[h]").astype(int).tolist()
        if args.leads == "all"
        else [int(x) for x in args.leads.split(",")]
    )
    metadata = {
        "model": args.model,
        "dataset": config["dataset"],
        "init_time": str(init),
        "cells": len(ids),
        "levels": levels.tolist(),
        "leads": leads,
        "variables": config["variables"],
        "surface_variables": SURFACE,
        "grid": grid,
        "workers": args.workers,
        "zarr_concurrency": args.concurrency,
        "open_seconds": time.perf_counter() - opening,
        "units": {v: ds[v].attrs.get("units") for v in config["variables"]},
        "coverage": args.coverage,
        "spatial_shards": args.spatial_shards,
        "shard_index": args.shard_index,
        "offline_cache": args.offline_cache,
    }
    metadata["snapshot_id"] = snapshot_id
    if metadata_path.exists():
        old = json.loads(metadata_path.read_text())
        defaults = {"coverage": "sample", "spatial_shards": 1, "shard_index": 0}
        if any(
            old.get(k, defaults.get(k)) != metadata.get(k)
            for k in [
                "init_time",
                "snapshot_id",
                "cells",
                "levels",
                "leads",
                "coverage",
                "spatial_shards",
                "shard_index",
            ]
        ):
            raise RuntimeError(
                "Output directory belongs to a different extraction; use a new directory"
            )
        if (args.output / "cells.npz").exists():
            np.testing.assert_array_equal(
                np.load(args.output / "cells.npz")["cell_id"], ids
            )
    metadata_path.write_text(json.dumps(metadata, indent=2))
    np.savez(args.output / "cells.npz", cell_id=ids, latitude=lat, longitude=lon)
    pressure = ds.sel(init_time=init)
    surface = root.sel(init_time=init)
    print(
        json.dumps(
            {
                k: metadata[k]
                for k in [
                    "model",
                    "init_time",
                    "snapshot_id",
                    "cells",
                    "levels",
                    "leads",
                    "open_seconds",
                ]
            }
        ),
        flush=True,
    )
    print(
        json.dumps(
            {"native_cells": grid["native_cells"], "conus_cells": grid["conus_cells"]}
        ),
        flush=True,
    )
    process = psutil.Process()
    for lead in leads:
        report_path = args.output / f"lead-{lead:03d}.json"
        if report_path.exists():
            if not all(
                (args.output / f"{prefix}-{lead:03d}.npy").exists()
                for prefix in ("lead", "surface")
            ):
                raise FileNotFoundError(
                    "Completed staging files were removed; use a new output directory"
                )
            print(f"reuse lead {lead}", flush=True)
            continue
        before = snapshot()
        cache_before = store.snapshot() if isinstance(store, SourceCache) else None
        retry_count = [0]
        net_start, cpu_start, io_start = (
            net_bytes(),
            time.process_time(),
            process.io_counters(),
        )
        started = time.perf_counter()

        def read(task, lead=lead, retry_count=retry_count):
            variable, level_index = task
            source = pressure if variable in config["variables"] else surface
            value = source[variable].sel(lead_time=np.timedelta64(lead, "h"))
            if level_index is not None:
                value = value.sel(pressure_level=levels[level_index])
            # Explicitly decode one map and gather all cells. Vectorized Xarray
            # indexing across several unsliced axes can materialize a full run.
            spatial = ("y", "x") if args.model == "hrrr" else ("latitude", "longitude")
            for attempt in range(4):
                try:
                    whole_map = value.transpose(*spatial).values
                    break
                except Exception:
                    if attempt == 3:
                        raise
                    with LOCK:
                        retry_count[0] += 1
                    time.sleep(2**attempt)
            data = whole_map[tuple(indexers[d].values for d in spatial)]
            small = data.astype(np.float32)
            finite = np.isfinite(data)
            error = (
                float(np.max(np.abs(data[finite] - small[finite])))
                if finite.any()
                else None
            )
            return (
                variable,
                level_index,
                small,
                {
                    "finite": int(finite.sum()),
                    "total": int(data.size),
                    "max_float32_error": error,
                },
            )

        tasks = [(v, i) for v in config["variables"] for i in range(len(levels))] + [
            (v, None) for v in SURFACE
        ]
        with ThreadPoolExecutor(max_workers=args.workers) as pool:
            results = list(pool.map(read, tasks))
        profile = np.empty(
            (len(ids), len(levels), len(config["variables"])), dtype=np.float32
        )
        ground = np.empty((len(ids), len(SURFACE)), dtype=np.float32)
        field_stats = {}
        for name, level_index, data, stats in results:
            if level_index is None:
                ground[:, SURFACE.index(name)] = data
            else:
                profile[:, level_index, config["variables"].index(name)] = data
            merged = field_stats.setdefault(
                name, {"finite": 0, "total": 0, "max_float32_error": 0.0}
            )
            merged["finite"] += stats["finite"]
            merged["total"] += stats["total"]
            merged["max_float32_error"] = max(
                merged["max_float32_error"], stats["max_float32_error"] or 0.0
            )
        if (
            shutil.disk_usage(args.output).free
            < profile.nbytes + ground.nbytes + 2 * 1024**3
        ):
            raise OSError("Staging this forecast time would leave less than 2 GiB free")
        np.save(args.output / f"lead-{lead:03d}.npy", profile)
        np.save(args.output / f"surface-{lead:03d}.npy", ground)
        after = snapshot()
        report = {
            "lead": lead,
            "workers": args.workers,
            "zarr_concurrency": args.concurrency,
            "seconds": time.perf_counter() - started,
            "cpu_seconds": time.process_time() - cpu_start,
            "grib_bytes": after["grib_bytes"] - before["grib_bytes"],
            "chunks": after["chunks"] - before["chunks"],
            "decode_seconds": after["decode_seconds"] - before["decode_seconds"],
            "network_rx_bytes": net_bytes() - net_start,
            "staged_value_bytes": profile.nbytes + ground.nbytes,
            "peak_rss_bytes": resource.getrusage(resource.RUSAGE_SELF).ru_maxrss * 1024,
            "process_write_bytes": process.io_counters().write_bytes
            - io_start.write_bytes,
            "fields": field_stats,
        }
        expected_chunks = len(levels) * len(config["variables"]) + len(SURFACE)
        if report["chunks"] != expected_chunks:
            raise RuntimeError(
                f"Expected {expected_chunks} distinct GRIB decodes; observed {report['chunks']}"
            )
        if not all(s["finite"] > 0 for s in field_stats.values()):
            raise RuntimeError(
                "A required field has no finite values; do not publish this extraction"
            )
        report["map_retries"] = retry_count[0]
        if cache_before is not None:
            cache_after = store.snapshot()
            report["source_cache"] = {
                k: cache_after[k] - cache_before[k] for k in cache_after
            }
        temporary = report_path.with_suffix(".tmp")
        temporary.write_text(json.dumps(report, indent=2))
        temporary.replace(report_path)
        print(json.dumps(report), flush=True)
        del results, profile, ground, data
    (args.output / "codec-totals.json").write_text(json.dumps(snapshot(), indent=2))


if __name__ == "__main__":
    main()
