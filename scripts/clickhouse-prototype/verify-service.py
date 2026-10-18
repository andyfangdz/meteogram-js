"""Verify retained rows and HTTP state; optionally exercise database restart.

--restart-database intentionally interrupts the local API until recovery completes.
It requires an idle updater and holds its lock throughout the check.
"""

import argparse
import json
import math
import time
import urllib.request
from pathlib import Path

from benchmark import client
from native_store import MODELS, atomic_json, catalog, descriptor_path, identifier, lock


def health():
    with urllib.request.urlopen("http://127.0.0.1:18080/health", timeout=2) as response:
        body = json.load(response)
    if set(body["models"]) != set(MODELS) or not all(
        value["available"] for value in body["models"].values()
    ):
        raise ValueError("A published model is unavailable")
    return body


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--restart-database", action="store_true")
    p.add_argument("--output", type=Path)
    args = p.parse_args()
    root = args.root.resolve()
    result = {"started": time.time(), "restart_requested": args.restart_database}
    with lock(root, "update", exclusive=True, nonblocking=True):
        before = catalog(root)
        schedule = json.loads((root / "schedule.json").read_text())
        state = json.loads((root / "service-status.json").read_text())
        if (
            "update" in state.get("children", {})
            or schedule["next_update"] < time.time() + 120
        ):
            raise RuntimeError("Wait for an idle updater and a later scheduled job")
        health()
        if args.restart_database:
            old_pid = state["children"]["clickhouse"]["pid"]
            started = time.monotonic()
            try:
                urllib.request.urlopen(
                    urllib.request.Request(
                        "http://127.0.0.1:18123/", data=b"SYSTEM SHUTDOWN"
                    ),
                    timeout=5,
                ).close()
            except OSError:
                pass  # The server can close the connection before replying.
            for _ in range(45):
                time.sleep(1)
                try:
                    state = json.loads((root / "service-status.json").read_text())
                    new_pid = state["children"]["clickhouse"]["pid"]
                    if old_pid == new_pid:
                        continue
                    health()
                    break
                except (OSError, KeyError):
                    continue
            else:
                raise RuntimeError("Automatic database/API recovery failed")
            result.update(
                database_restart_recovery_seconds=time.monotonic() - started,
                old_database_pid=old_pid,
                new_database_pid=new_pid,
            )
        c = client()
        try:
            counts = {}
            for section in ("current", "previous"):
                for model, entry in before[section].items():
                    meta = json.loads(
                        (descriptor_path(root, entry) / "metadata.json").read_text()
                    )
                    count = c.query(
                        f"SELECT count() FROM {identifier(entry['database'])}.{identifier(entry['table'])}"
                    ).first_row[0]
                    if count != meta["cells"] * len(meta["leads"]):
                        raise ValueError("Retained row count changed after restart")
                    counts[f"{section}/{model}"] = count
            result["retained_rows"] = counts
            schema = c.query("SHOW CREATE TABLE system.query_log").first_row[0]
            if "TTL" not in schema or "toIntervalDay(7)" not in schema:
                raise ValueError("Query log does not have the expected seven-day TTL")
            result["query_log_ttl_days"] = 7
        finally:
            c.close()
        for model, entry in before["current"].items():
            meta = json.loads(
                (descriptor_path(root, entry) / "metadata.json").read_text()
            )
            with urllib.request.urlopen(
                f"http://127.0.0.1:18080/forecast?model={model}&lat=33.6407&lon=-84.4277",
                timeout=15,
            ) as response:
                body = json.load(response)
            if body["revision"] != entry["revision"] or any(
                body[key] != meta[key] for key in ("init_time", "snapshot_id")
            ):
                raise ValueError("HTTP revision changed unexpectedly")
            if [row[0] for row in body["rows"]] != meta["leads"]:
                raise ValueError("Incomplete HTTP forecast")
            for row in body["rows"]:
                for values in row[1:-2]:
                    if len(values) != len(meta["levels"]) or not all(
                        math.isfinite(value) for value in values
                    ):
                        raise ValueError("Invalid HTTP pressure array")
                if not all(math.isfinite(value) for value in row[-2:]):
                    raise ValueError("Invalid surface values")
        if (
            catalog(root) != before
            or json.loads((root / "schedule.json").read_text()) != schedule
        ):
            raise ValueError("Catalog or refresh schedule changed during restart")
        result.update(
            ended=time.time(),
            catalog_preserved=True,
            schedule_preserved=True,
            health=health(),
        )
    atomic_json(args.output or root / "service-verification.json", result)
    print(json.dumps(result, indent=2))


if __name__ == "__main__":
    main()
