"""Replay a pinned serving cycle into a separate catalog while observing its API.

Downloads real source data again; never publishes into the serving catalog.
Use an empty output directory. Full-table validation and source-profile equality
are required. Candidate tables remain for inspection until explicit cleanup.
"""

import argparse
import gzip
import json
import shutil
import threading
import time
import urllib.request
from pathlib import Path

import numpy as np
import pipeline
from benchmark import client, distribution, query_profile
from native_store import MODELS, atomic_json, catalog, descriptor_path, lock


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--source-root", type=Path, required=True)
    p.add_argument("--output", type=Path, required=True)
    args = p.parse_args()
    if args.output.exists():
        p.error("Use a new output directory")
    args.output.mkdir(parents=True)
    with (
        lock(args.source_root, "update", exclusive=True, nonblocking=True),
        lock(args.output, "update", exclusive=True, nonblocking=True),
    ):
        original = catalog(args.source_root)
        original_fetch = pipeline.run_fetch

        def pinned_fetch(
            root, path, model, offset, metadata_only=False, cancelled=None
        ):
            if metadata_only:
                source = descriptor_path(args.source_root, original["current"][model])
                for name in ("metadata.json", "cells.npz"):
                    shutil.copyfile(source / name, path / name)
                return
            return original_fetch(root, path, model, offset, cancelled=cancelled)

        pipeline.run_fetch = pinned_fetch
        stop = threading.Event()
        observations, errors, free = [], [], []

        def observe():
            while not stop.is_set():
                free.append(shutil.disk_usage(args.output).free)
                for model in MODELS:
                    started = time.perf_counter()
                    try:
                        request = urllib.request.Request(
                            f"http://127.0.0.1:18080/forecast?model={model}&lat=40.73&lon=-73.42",
                            headers={"Accept-Encoding": "gzip"},
                        )
                        with urllib.request.urlopen(request, timeout=15) as response:
                            payload = response.read()
                            if response.headers.get("Content-Encoding") == "gzip":
                                payload = gzip.decompress(payload)
                            body = json.loads(payload)
                        if body["revision"] != original["current"][model]["revision"]:
                            raise ValueError(
                                "Serving revision changed during isolated benchmark"
                            )
                        observations.append(
                            {
                                "model": model,
                                "time": time.time(),
                                "ms": (time.perf_counter() - started) * 1000,
                                "query_ms": body["query_ms"],
                            }
                        )
                    except Exception as error:  # noqa: BLE001 -- record availability failures
                        errors.append(
                            {"model": model, "time": time.time(), "error": str(error)}
                        )
                stop.wait(2)

        observer = threading.Thread(target=observe)
        observer.start()
        c = None

        def counters():
            return dict(
                c.query(
                    "SELECT event,value FROM system.events WHERE event IN ('OSWriteBytes','OSReadBytes')"
                ).result_rows
            )

        try:
            c = client()
            before, started = counters(), time.time()
            exit_code = pipeline.update(args.output, list(MODELS), [1])
            ended = time.time()
            after = counters()
            if exit_code:
                raise RuntimeError("Full update failed; inspect events and source logs")
            state = catalog(args.output)
            comparisons, query_times = {}, {}
            for model in MODELS:
                entry = state["current"][model]
                source = original["current"][model]
                source_meta = json.loads(
                    (
                        descriptor_path(args.source_root, source) / "metadata.json"
                    ).read_text()
                )
                path = descriptor_path(args.output, entry)
                meta = json.loads((path / "metadata.json").read_text())
                ids = np.load(path / "cells.npz")["cell_id"]
                samples = np.random.default_rng(17).choice(ids, 100, replace=False)
                fields = meta["variables"] + meta["surface_variables"]
                latency = []
                for cell in samples:
                    actual, ms = query_profile(
                        c,
                        entry["table"],
                        fields,
                        cell,
                        database=entry["database"],
                        profile=meta,
                    )
                    expected, _ = query_profile(
                        c,
                        source["table"],
                        fields,
                        cell,
                        database=source["database"],
                        profile=source_meta,
                    )
                    if actual.result_rows != expected.result_rows:
                        raise ValueError(f"Source-profile mismatch: {model}/{cell}")
                    latency.append(ms)
                comparisons[model] = len(samples)
                query_times[model] = distribution(latency)
            if catalog(args.source_root) != original:
                raise ValueError("Serving catalog changed")
            atomic_json(
                args.output / "complete.json",
                {
                    "started": started,
                    "ended": ended,
                    "seconds": ended - started,
                    "source_profile_comparisons": comparisons,
                    "candidate_query_ms": query_times,
                    "server_events_delta": {k: after[k] - before[k] for k in before},
                },
            )
        finally:
            stop.set()
            observer.join(timeout=50)
            if c is not None:
                c.close()
            atomic_json(args.output / "http-observations.json", observations)
            atomic_json(
                args.output / "http-summary.json",
                {
                    "requests": len(observations),
                    "errors": errors,
                    "min_free_bytes": min(free) if free else None,
                    "latency_ms": {
                        model: distribution(
                            [o["ms"] for o in observations if o["model"] == model]
                        )
                        for model in MODELS
                        if any(o["model"] == model for o in observations)
                    },
                },
            )


if __name__ == "__main__":
    main()
