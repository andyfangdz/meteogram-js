"""Operate local native forecasts: adopt, update, rollback, status and GC."""

import argparse
import json
import os
import shutil
import signal
import subprocess
import sys
import threading
import time
import uuid
from concurrent.futures import ThreadPoolExecutor
from pathlib import Path

from benchmark import client
from native_store import (
    ADMISSION,
    DATABASE,
    LEADS,
    MODELS,
    atomic_json,
    catalog,
    check_metadata,
    clean_workspace,
    collect,
    descriptor_path,
    disk_guard,
    freshness,
    identifier,
    load,
    lock,
    publish,
    timestamp,
    validate,
)

SCRIPTS = Path(__file__).resolve().parent
EVENT_LOCK = threading.Lock()


def event(root, **value):
    value = {"time": time.time(), **value}
    with EVENT_LOCK:
        print(json.dumps(value), flush=True)
        path = root / "events.jsonl"
        if path.exists() and path.stat().st_size > 20 * 1024**2:
            path.replace(root / "events-previous.jsonl")
        with path.open("a") as out:
            out.write(json.dumps(value) + "\n")


def run_fetch(root, path, model, offset, metadata_only=False, cancelled=None):
    command = [
        sys.executable,
        "-u",
        str(SCRIPTS / "fetch.py"),
        "--model",
        model,
        "--coverage",
        "conus",
        "--leads",
        ",".join(map(str, LEADS[model])),
        "--run-offset",
        str(offset),
        "--workers",
        "16",
        "--concurrency",
        "16",
        "--cache-dir",
        str(path / "source-cache"),
        "--output",
        str(path),
    ]
    if metadata_only:
        command.append("--metadata-only")
    with (path / "fetch.log").open("a") as log:
        child = subprocess.Popen(
            command, stdout=log, stderr=subprocess.STDOUT, start_new_session=True
        )
        started = time.monotonic()
        try:
            while child.poll() is None:
                if cancelled is not None and cancelled.is_set():
                    raise InterruptedError("Update cancelled")
                disk_guard(root)
                if time.monotonic() - started > 2700:
                    raise TimeoutError("Source extraction exceeded 45 minutes")
                time.sleep(1)
            if child.returncode:
                raise RuntimeError(
                    f"Source extraction failed ({child.returncode}); see {path / 'fetch.log'}"
                )
        finally:
            if child.poll() is None:
                os.killpg(child.pid, signal.SIGTERM)
                try:
                    child.wait(timeout=10)
                except subprocess.TimeoutExpired:
                    os.killpg(child.pid, signal.SIGKILL)
                    child.wait()


def referenced(root):
    state = catalog(root)
    return {
        e["revision"]
        for section in ("current", "previous")
        for e in state[section].values()
    } | {e["revision"] for e in state["retired"]}


def discard(root, path):
    if path.name in referenced(root) or (path / "discarded.json").exists():
        return
    entry_file = path / "entry.json"
    if entry_file.exists():
        entry = json.loads(entry_file.read_text())
        if entry.get("owned"):
            c = client()
            try:
                c.command(
                    f"DROP TABLE IF EXISTS {identifier(entry['database'])}.{identifier(entry['table'])} SYNC"
                )
            finally:
                c.close()
    clean_workspace(path)
    (path / "cells.npz").unlink(missing_ok=True)
    atomic_json(path / "discarded.json", {"time": time.time()})


def recover(root):
    # After any crash, immutable published tables stay intact. An unpublished
    # partial insert is dropped and rebuilt in a fresh revision, never appended.
    keep = referenced(root)
    for path in (root / "revisions").glob("*"):
        if path.is_dir() and path.name not in keep:
            discard(root, path)
            if (
                time.time() - json.loads((path / "discarded.json").read_text())["time"]
                > 7 * 86400
            ):
                shutil.rmtree(path)
        elif path.is_dir():
            clean_workspace(path)


def candidate(root, model, owned=True):
    revision = (
        f"{model}_{time.strftime('%Y%m%d%H%M%S', time.gmtime())}_{uuid.uuid4().hex[:8]}"
    )
    path = root / "revisions" / revision
    path.mkdir(parents=True)
    entry = {
        "revision": revision,
        "database": DATABASE,
        "table": revision,
        "owned": owned,
    }
    atomic_json(path / "entry.json", entry)
    return path, entry


def clone_import(root, source_path, source_database, source_table, model):
    path, entry = candidate(root, model)
    try:
        for filename in ("metadata.json", "cells.npz"):
            shutil.copy2(source_path / filename, path / filename)
        c = client()
        try:
            c.command(f"CREATE DATABASE IF NOT EXISTS {DATABASE}")
            c.command(
                f"CREATE TABLE {DATABASE}.{identifier(entry['table'])} CLONE AS {identifier(source_database)}.{identifier(source_table)}"
            )
            c.command(
                f"ALTER TABLE {DATABASE}.{identifier(entry['table'])} MODIFY SETTING old_parts_lifetime=60"
            )
        finally:
            c.close()
        validate(path, entry["table"])
        return path, entry
    except BaseException:
        discard(root, path)
        raise


def isolate_imports(root):
    # Migrate early deployments that borrowed benchmark tables. Clone first,
    # validate outside the reader lock, then replace only that exact reference.
    for section in ("current", "previous"):
        for model, old in catalog(root)[section].items():
            if old.get("owned"):
                continue
            path, entry = clone_import(
                root, descriptor_path(root, old), old["database"], old["table"], model
            )
            with lock(root, exclusive=True):
                state = catalog(root)
                if state[section].get(model) != old:
                    raise RuntimeError("Catalog changed during import isolation")
                state[section][model] = entry
                state["retired"].append({**old, "retired_at": time.time()})
                state["generation"] += 1
                atomic_json(root / "catalog.json", state)
            event(
                root,
                phase="isolated_import",
                model=model,
                section=section,
                revision=path.name,
            )


def prepare(root, model, offsets, cancelled):
    """Pin and stage one model; publication remains on the updater thread."""
    errors = []
    for offset in offsets:
        if cancelled.is_set():
            raise InterruptedError("Update cancelled")
        path, entry = candidate(root, model)
        try:
            event(root, phase="select", model=model, revision=path.name, offset=offset)
            run_fetch(
                root, path, model, offset, metadata_only=True, cancelled=cancelled
            )
            meta = json.loads((path / "metadata.json").read_text())
            check_metadata(meta)
            active = catalog(root)["current"].get(model)
            if active:
                old = json.loads(
                    (descriptor_path(root, active) / "metadata.json").read_text()
                )
                if timestamp(meta["init_time"]) <= timestamp(old["init_time"]):
                    if not freshness(old)["available"]:
                        raise ValueError(
                            "No newer source and the current forecast is stale"
                        )
                    event(
                        root,
                        phase="unchanged",
                        model=model,
                        init_time=meta["init_time"],
                    )
                    discard(root, path)
                    return None
            if not freshness(meta)["available"]:
                raise ValueError("Selected source fails freshness/horizon policy")
            disk_guard(root, ADMISSION[model])
            event(root, phase="fetch", model=model, init_time=meta["init_time"])
            run_fetch(root, path, model, offset, cancelled=cancelled)
            # Reject incomplete source values here so offset fallback still
            # happens before the loader and a prefetched candidate is usable.
            for lead in meta["leads"]:
                report = json.loads((path / f"lead-{lead:03d}.json").read_text())
                if not report["fields"] or any(
                    field["finite"] != field["total"]
                    for field in report["fields"].values()
                ):
                    raise ValueError("Missing source values: try an older complete run")
            event(root, phase="staged", model=model, revision=path.name)
            return path, entry, meta
        except Exception as error:
            event(
                root, phase="failed", model=model, revision=path.name, error=str(error)
            )
            atomic_json(
                path / "failed.json", {"time": time.time(), "error": str(error)}
            )
            discard(root, path)
            if cancelled.is_set():
                raise
            errors.append(str(error))
    raise RuntimeError("; ".join(errors))


def update(root, models, offsets, prefetch=True):
    collect(root)
    recover(root)
    started = time.time()
    failures = []
    cancelled = threading.Event()
    pool = ThreadPoolExecutor(max_workers=1, thread_name_prefix="source-prefetch")
    future = None
    try:
        for index, model in enumerate(models):
            pending, future = future, None
            # Start one model ahead before waiting for the current source.
            # This permits two extractors during initial acquisition, then
            # one extractor alongside the loader. Both full disk budgets are
            # admitted together; a single worker bounds queued prefetches.
            if prefetch and index + 1 < len(models):
                next_model = models[index + 1]
                try:
                    disk_guard(root, ADMISSION[model] + ADMISSION[next_model])
                except OSError:
                    event(
                        root,
                        phase="prefetch_deferred",
                        model=next_model,
                        reason="disk_budget",
                    )
                else:
                    future = pool.submit(prepare, root, next_model, offsets, cancelled)
            try:
                prepared = (
                    pending.result()
                    if pending
                    else prepare(root, model, offsets, cancelled)
                )
            except Exception as error:  # noqa: BLE001 -- another model can still advance
                failures.append({"model": model, "error": str(error)})
                continue
            if prepared is None:
                continue
            path, entry, meta = prepared
            try:
                event(root, phase="load", model=model)
                load(root, path, entry["table"])
                state = publish(root, model, entry)
                event(
                    root,
                    phase="published",
                    model=model,
                    init_time=meta["init_time"],
                    generation=state["generation"],
                    revision=path.name,
                )
                clean_workspace(path)
            except Exception as error:  # noqa: BLE001 -- failed candidates never publish
                event(
                    root,
                    phase="failed",
                    model=model,
                    revision=path.name,
                    error=str(error),
                )
                atomic_json(
                    path / "failed.json", {"time": time.time(), "error": str(error)}
                )
                discard(root, path)
                failures.append({"model": model, "error": str(error)})
    finally:
        cancelled.set()
        pool.shutdown(wait=True, cancel_futures=True)
    collect(root)
    atomic_json(
        root / "last-update.json",
        {
            "started": started,
            "time": time.time(),
            "failures": failures,
            "prefetch": prefetch,
        },
    )
    return 1 if failures else 0


def main():
    def terminate(signum, frame):
        raise SystemExit(128 + signum)

    signal.signal(signal.SIGTERM, terminate)
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, required=True)
    sub = p.add_subparsers(dest="command", required=True)
    adopt = sub.add_parser("adopt")
    adopt.add_argument("--benchmark-root", type=Path, required=True)
    updater = sub.add_parser("update")
    updater.add_argument("--models", default=",".join(MODELS))
    updater.add_argument("--offsets", default="1,2")
    updater.add_argument("--no-prefetch", action="store_true")
    rollback = sub.add_parser("rollback")
    rollback.add_argument("--model", choices=MODELS, required=True)
    sub.add_parser("gc")
    sub.add_parser("isolate-imports")
    sub.add_parser("status")
    sub.add_parser("stop")
    args = p.parse_args()
    root = args.root.resolve()
    root.mkdir(parents=True, exist_ok=True)
    if args.command == "stop":
        atomic_json(root / "stop-request.json", {"time": time.time()})
        print("Supervisor stop requested")
        return
    if args.command == "status":
        with lock(root):
            state = catalog(root)
            state["freshness"] = {
                model: freshness(
                    json.loads(
                        (descriptor_path(root, entry) / "metadata.json").read_text()
                    )
                )
                for model, entry in state["current"].items()
            }
        print(json.dumps(state, indent=2))
        return
    with lock(root, "update", exclusive=True, nonblocking=True):
        if args.command == "adopt":
            if not (args.benchmark_root / "complete.json").exists():
                raise ValueError("Adoption requires the completed benchmark marker")
            for model in MODELS:
                if model in catalog(root)["current"]:
                    continue
                _path, entry = clone_import(
                    root,
                    args.benchmark_root / model,
                    "meteogram_bench",
                    f"{model}_256_zstd",
                    model,
                )
                publish(root, model, entry)
                event(root, phase="adopted", model=model)
        elif args.command == "update":
            models = args.models.split(",")
            offsets = [int(value) for value in args.offsets.split(",")]
            if (
                not models
                or len(set(models)) != len(models)
                or any(model not in MODELS for model in models)
                or not offsets
                or min(offsets) < 1
            ):
                p.error("Invalid models or offsets")
            sys.exit(update(root, models, offsets, prefetch=not args.no_prefetch))
        elif args.command == "isolate-imports":
            isolate_imports(root)
        elif args.command == "rollback":
            entry = catalog(root)["previous"].get(args.model)
            if not entry:
                raise ValueError("No previous revision available")
            publish(root, args.model, entry, rollback=True)
            event(root, phase="rollback", model=args.model, revision=entry["revision"])
        else:
            collect(root)
            recover(root)


if __name__ == "__main__":
    main()
