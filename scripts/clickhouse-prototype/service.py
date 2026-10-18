"""Supervise local ClickHouse/API and periodic forecast updates.

Use with systemd --user. Run directly when the service manager is unavailable.
Only loopback sockets are exposed; the web application's provider is unchanged.
"""

import argparse
import json
import math
import os
import signal
import socket
import subprocess
import sys
import time
import urllib.request
from pathlib import Path

from native_store import atomic_json, lock

SCRIPTS = Path(__file__).resolve().parent


def main():
    p = argparse.ArgumentParser()
    p.add_argument("--root", type=Path, required=True)
    p.add_argument("--server-root", type=Path, required=True)
    p.add_argument("--binary", type=Path, required=True)
    p.add_argument("--interval-hours", type=float, default=6)
    args = p.parse_args()
    if not math.isfinite(args.interval_hours) or args.interval_hours <= 0:
        p.error("interval-hours must be finite and positive")
    args.root.mkdir(parents=True, exist_ok=True)
    children = {}
    logs = {}
    stopping = False

    def stop_signal(signum, frame):
        nonlocal stopping
        stopping = True

    signal.signal(signal.SIGTERM, stop_signal)
    signal.signal(signal.SIGINT, stop_signal)

    def start(name, command):
        log = args.root / f"{name}.log"
        if log.exists() and log.stat().st_size > 20 * 1024**2:
            log.replace(log.with_suffix(".previous.log"))
        if name in logs:
            logs[name].close()
        logs[name] = log.open("a")
        children[name] = subprocess.Popen(
            command,
            stdout=logs[name],
            stderr=subprocess.STDOUT,
            env={**os.environ, "CLICKHOUSE_WATCHDOG_ENABLE": "0"},
            # ClickHouse SYSTEM SHUTDOWN may signal its process group. Keep
            # that signal away from this supervisor and the other children.
            start_new_session=True,
        )

    def alive(name):
        return name in children and children[name].poll() is None

    with lock(args.root, "service", exclusive=True, nonblocking=True):
        # Never attach to an unknown instance: this supervisor owns its server.
        for port in (18123, 19000, 18080):
            with socket.socket() as sock:
                sock.settimeout(1)
                if sock.connect_ex(("127.0.0.1", port)) == 0:
                    raise RuntimeError(
                        f"Loopback port {port} already occupied; stop the existing launcher first"
                    )
        stop_request = args.root / "stop-request.json"
        stop_request.unlink(missing_ok=True)
        next_update = 0
        schedule = args.root / "schedule.json"
        if schedule.exists():
            next_update = json.loads(schedule.read_text()).get("next_update", 0)
        try:
            while not stopping:
                if stop_request.exists():
                    stop_request.unlink()
                    break
                if not alive("clickhouse"):
                    # Restart all users of the old DB connection after a crash.
                    for name in ("api", "update"):
                        if alive(name):
                            children[name].terminate()
                            children[name].wait(timeout=30)
                    start(
                        "clickhouse",
                        [
                            sys.executable,
                            str(SCRIPTS / "start-server.py"),
                            "--binary",
                            str(args.binary),
                            "--work-dir",
                            str(args.server_root),
                        ],
                    )
                    for _ in range(60):
                        if stopping:
                            break
                        try:
                            urllib.request.urlopen(
                                "http://127.0.0.1:18123/ping", timeout=1
                            ).close()
                            break
                        except OSError:
                            time.sleep(1)
                    else:
                        raise RuntimeError("ClickHouse did not start within 60 seconds")
                if not alive("api") and not stopping:
                    start(
                        "api",
                        [
                            sys.executable,
                            "-u",
                            str(SCRIPTS / "serve.py"),
                            "--catalog-root",
                            str(args.root),
                        ],
                    )
                if "update" in children and not alive("update"):
                    result = children.pop("update").returncode
                    next_update = time.time() + (
                        args.interval_hours * 3600 if result == 0 else 1800
                    )
                    atomic_json(
                        schedule, {"next_update": next_update, "last_exit": result}
                    )
                if time.time() >= next_update and not alive("update") and not stopping:
                    start(
                        "update",
                        [
                            sys.executable,
                            "-u",
                            str(SCRIPTS / "pipeline.py"),
                            "--root",
                            str(args.root),
                            "update",
                        ],
                    )
                atomic_json(
                    args.root / "service-status.json",
                    {
                        "time": time.time(),
                        "next_update": next_update,
                        "children": {
                            name: {"pid": child.pid, "running": child.poll() is None}
                            for name, child in children.items()
                        },
                    },
                )
                time.sleep(2)
        finally:
            # API/updater before DB; their shared catalog locks are released by exit.
            for name in ("update", "api", "clickhouse"):
                if alive(name):
                    children[name].terminate()
                    try:
                        children[name].wait(timeout=30)
                    except subprocess.TimeoutExpired:
                        children[name].kill()
                        children[name].wait()
            for log in logs.values():
                log.close()
            try:
                atomic_json(
                    args.root / "service-status.json",
                    {"time": time.time(), "stopped": True, "next_update": next_update},
                )
            except OSError:
                pass  # Cleanup must still release the service lock on disk failure.


if __name__ == "__main__":
    main()
