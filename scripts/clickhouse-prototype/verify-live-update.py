"""Observe real HTTP availability and profile stability through publication."""

import argparse
import gzip
import hashlib
import json
import time
import urllib.error
import urllib.request
from pathlib import Path

import numpy as np
from native_store import MODELS, atomic_json


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--output", type=Path, required=True)
    p.add_argument("--timeout", type=int, default=7200)
    args = p.parse_args()
    started = time.time()
    first = {}
    seen = {model: set() for model in MODELS}
    hashes = {}
    initial_times = {}
    advanced = set()
    observations = []
    errors = []
    locations = [
        (33.6407, -84.4277),
        (47.45, -122.31),
        (25.79, -80.29),
        (39.85, -104.67),
    ]
    iteration = 0
    while time.time() - started < args.timeout:
        for model in MODELS:
            lat, lon = locations[iteration % len(locations)]
            begin = time.perf_counter()
            try:
                request = urllib.request.Request(
                    f"http://127.0.0.1:18080/forecast?model={model}&lat={lat}&lon={lon}",
                    headers={"Accept-Encoding": "gzip"},
                )
                with urllib.request.urlopen(request, timeout=15) as response:
                    data = response.read()
                    if response.headers.get("Content-Encoding") == "gzip":
                        data = gzip.decompress(data)
                    body = json.loads(data)
                revision = body["revision"]
                first.setdefault(model, revision)
                initial_times.setdefault(model, body["init_time"])
                if body["init_time"] > initial_times[model]:
                    advanced.add(model)
                seen[model].add(revision)
                rows = body["rows"]
                if len({row[0] for row in rows}) != len(rows):
                    raise ValueError("Duplicate lead")
                if body["model"] != model or not rows:
                    raise ValueError("Incorrect model or empty profile")
                stable = {
                    key: body[key]
                    for key in (
                        "init_time",
                        "snapshot_id",
                        "grid",
                        "levels_hpa",
                        "fields",
                        "rows",
                    )
                }
                digest = hashlib.sha256(
                    json.dumps(stable, sort_keys=True).encode()
                ).hexdigest()
                key = (model, revision, lat, lon)
                if key in hashes and hashes[key] != digest:
                    raise ValueError("Profile changed within immutable revision")
                hashes[key] = digest
                observations.append(
                    {
                        "time": time.time(),
                        "model": model,
                        "revision": revision,
                        "init_time": body["init_time"],
                        "snapshot_id": body["snapshot_id"],
                        "leads": len(rows),
                        "ms": (time.perf_counter() - begin) * 1000,
                        "query_ms": body["query_ms"],
                    }
                )
            except (OSError, ValueError, KeyError) as error:
                errors.append(
                    {"time": time.time(), "model": model, "error": str(error)}
                )
        iteration += 1
        if iteration % 15 == 0 or len(advanced) == len(MODELS):
            summary = {
                "started": started,
                "ended": time.time(),
                "requests": len(observations),
                "errors": errors,
                "revisions": {model: sorted(values) for model, values in seen.items()},
                "all_models_updated": len(advanced) == len(MODELS),
                "latency_ms": {
                    model: {
                        "p50": float(
                            np.percentile(
                                [
                                    row["ms"]
                                    for row in observations
                                    if row["model"] == model
                                ],
                                50,
                            )
                        ),
                        "p95": float(
                            np.percentile(
                                [
                                    row["ms"]
                                    for row in observations
                                    if row["model"] == model
                                ],
                                95,
                            )
                        ),
                        "max": max(
                            row["ms"] for row in observations if row["model"] == model
                        ),
                    }
                    for model in MODELS
                    if any(row["model"] == model for row in observations)
                },
            }
            atomic_json(args.output, summary)
            atomic_json(
                args.output.with_name(args.output.stem + "-observations.json"),
                observations,
            )
            print(
                json.dumps(
                    {
                        "requests": len(observations),
                        "errors": len(errors),
                        "revisions": summary["revisions"],
                    }
                ),
                flush=True,
            )
        if len(advanced) == len(MODELS):
            return 1 if errors else 0
        time.sleep(2)
    raise TimeoutError("Did not observe publication for all three models")


if __name__ == "__main__":
    raise SystemExit(main())
