# /// script
# requires-python = ">=3.12"
# dependencies = [
#   "icechunk==2.3.0",
#   "gribberish==1.8.4",
#   "xarray==2026.9.0",
#   "zarr==3.4.1",
#   "pyproj==3.8.0",
# ]
# ///
"""Benchmark a single meteogram column from Dynamical's live Zarr catalog.

Run with `uv run scripts/probe-dynamical.py --model gfs`. This is deliberately
bounded to one forecast time: the virtual stores decode whole spatial maps even
when selecting one point. It does not change the app's weather provider.
"""

import argparse
import concurrent.futures
import json
import math
from pathlib import Path
import time
import urllib.request

import icechunk
import numpy as np
import xarray as xr
import zarr
from pyproj import CRS, Proj, Transformer


MODELS = {
    "gfs": {
        "dataset": "noaa-gfs-forecast-virtual",
        "variables": [
            "cloud_cover", "geopotential_height", "temperature",
            "wind_u", "wind_v", "relative_humidity",
        ],
        "forecast_steps": 137,  # Native 0–168h: hourly to 120h, then 3-hourly.
        "cloud_method": "model cloud cover; derive dew point from relative humidity",
    },
    "hrrr": {
        "dataset": "noaa-hrrr-forecast-48-hour-virtual",
        "variables": [
            "geopotential_height", "temperature", "wind_u", "wind_v",
            "relative_humidity", "dew_point_temperature",
        ],
        "forecast_steps": 40,
        "cloud_method": "estimate cloud cover from relative humidity",
    },
    "aifs": {
        "dataset": "ecmwf-aifs-single-forecast-virtual",
        "variables": [
            "geopotential_height", "temperature", "wind_u", "wind_v",
            "specific_humidity",
        ],
        "forecast_steps": 29,  # Native 0–168h at 6-hour intervals.
        "cloud_method": "derive humidity/dew point; estimate cloud cover from humidity",
    },
}


def open_store(dataset, snapshot_id=None, catalog_path=None):
    url = f"https://stac.dynamical.org/{dataset}/collection.json"
    request = urllib.request.Request(
        url, headers={"User-Agent": "meteogram-js-dynamical-probe/1.0"}
    )
    if catalog_path is not None and Path(catalog_path).exists():
        collection = json.loads(Path(catalog_path).read_text())
    else:
        with urllib.request.urlopen(request, timeout=30) as response:
            collection = json.load(response)
        if catalog_path is not None:
            Path(catalog_path).write_text(json.dumps(collection))
    asset = collection["assets"]["icechunk-https"]
    credentials = {}
    for container in asset.get("icechunk:virtual_chunk_containers", []):
        kind = container["credentials"]["type"]
        if kind == "s3":
            credential = icechunk.s3_anonymous_credentials()
        elif kind == "gcs":
            credential = icechunk.gcs_credentials(anonymous=True)
        else:
            raise ValueError(f"Unsupported virtual chunk storage: {kind}")
        credentials[container["url_prefix"]] = credential
    repository = icechunk.Repository.open(
        icechunk.http_storage(asset["href"]),
        authorize_virtual_chunk_access=icechunk.containers_credentials(credentials),
    )
    # One snapshot keeps all fields and coordinates on the same model revision.
    if snapshot_id is not None:
        return repository.readonly_session(snapshot_id=snapshot_id).store
    return repository.readonly_session("main").store


def select_point(dataset, latitude, longitude):
    if "x" not in dataset.dims:
        # Use circular distance so locations near +180 select across the seam.
        longitude = (longitude + 180) % 360 - 180
        distance = np.abs((dataset.longitude.values - longitude + 180) % 360 - 180)
        point = dataset.isel(longitude=int(distance.argmin())).sel(
            latitude=latitude, method="nearest"
        )
        return point, {
            "latitude": float(point.latitude), "longitude": float(point.longitude),
            "wind_rotation_degrees": 0,
        }

    crs = CRS.from_cf(dataset.spatial_ref.attrs)
    projection = Transformer.from_crs("EPSG:4326", crs, always_xy=True)
    x, y = projection.transform(longitude, latitude)
    for name, value in (("x", x), ("y", y)):
        coordinates = dataset[name].values
        if not np.isfinite(value) or not coordinates.min() <= value <= coordinates.max():
            raise ValueError("Location is outside the HRRR grid")
    point = dataset.sel(x=x, y=y, method="nearest")
    lon, lat = projection.transform(float(point.x), float(point.y), direction="INVERSE")
    convergence = Proj(crs).get_factors(lon, lat).meridian_convergence
    return point, {
        "latitude": lat, "longitude": lon, "x": float(point.x), "y": float(point.y),
        "wind_rotation_degrees": convergence,
    }


def probe(args):
    config = MODELS[args.model]
    started = time.monotonic()
    store = open_store(config["dataset"])
    dataset = xr.open_zarr(store, group="pressure_level", chunks=None, consolidated=False)
    point, grid = select_point(dataset, args.latitude, args.longitude)
    # This probe reports the latest run, which can still be ingesting. An adapter
    # must additionally verify completeness and use an older run when necessary.
    point = point.isel(init_time=-1).sel(lead_time=np.timedelta64(args.lead_hour, "h"))
    levels = dataset.pressure_level.values
    levels = np.sort(levels[(levels >= 275) & (levels <= 1000)])[::-1]
    if args.levels:
        levels = levels[:args.levels]
    point = point.sel(pressure_level=levels)
    open_seconds = time.monotonic() - started

    def read(name):
        started = time.monotonic()
        variable = point[name]
        values = variable.values
        finite = values[np.isfinite(values)]
        return {
            "variable": name, "units": variable.attrs.get("units"),
            "seconds": round(time.monotonic() - started, 3),
            "finite_levels": int(finite.size),
            "min": float(finite.min()) if finite.size else None,
            "max": float(finite.max()) if finite.size else None,
        }

    started = time.monotonic()
    # Bound both variable reads and Zarr chunk fan-out to control peak memory.
    with concurrent.futures.ThreadPoolExecutor(max_workers=2) as pool:
        results = list(pool.map(read, config["variables"]))
    read_seconds = time.monotonic() - started
    chunks = dataset.temperature.encoding["chunks"]
    chunk_bytes = math.prod(chunks) * dataset.temperature.dtype.itemsize
    sample_chunks = len(levels) * len(config["variables"])
    report = {
        "dataset": config["dataset"],
        "catalog": f"https://dynamical.org/catalog/{config['dataset']}",
        "init_time": str(point.init_time.values), "lead_hour": args.lead_hour,
        "requested_point": {"latitude": args.latitude, "longitude": args.longitude},
        "grid_point": grid, "pressure_levels_hpa": levels.tolist(),
        "cloud_method": config["cloud_method"],
        "open_seconds": round(open_seconds, 3),
        "column_read_seconds": round(read_seconds, 3),
        "variables": results,
        "temperature_chunk_shape": list(chunks),
        "temperature_chunk_decoded_mib": round(chunk_bytes / 2**20, 2),
        "column_chunks": sample_chunks,
        "representative_forecast_steps": config["forecast_steps"],
        "representative_forecast_decoded_gib": round(
            chunk_bytes * sample_chunks * config["forecast_steps"] / 2**30, 2
        ),
        "volume_note": "Decoded arrays, not network bytes or peak memory; excludes surface fields.",
    }
    dataset.close()
    print(json.dumps(report, indent=2, allow_nan=False))


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--model", choices=MODELS, default="gfs")
    parser.add_argument("--latitude", type=float, default=40.872665)
    parser.add_argument("--longitude", type=float, default=-74.283767)
    parser.add_argument("--lead-hour", type=int, default=6)
    parser.add_argument("--levels", type=int, help="Limit the number of sampled pressure levels")
    args = parser.parse_args()
    if not -90 <= args.latitude <= 90 or not -180 <= args.longitude <= 180:
        parser.error("Coordinates must be finite and within latitude/longitude bounds")
    if args.levels is not None and args.levels < 1:
        parser.error("--levels must be positive")
    if args.lead_hour < 0:
        parser.error("--lead-hour must be nonnegative")
    zarr.config.set({"async.concurrency": 4})
    probe(args)
