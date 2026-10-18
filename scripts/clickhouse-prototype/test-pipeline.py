"""Publication, crash recovery, retention, freshness and real DB loader tests."""

import json
import os
import tempfile
import threading
import time
import unittest
import uuid
from datetime import UTC, datetime
from pathlib import Path
from unittest.mock import patch

import numpy as np
from benchmark import client
from clickhouse_connect.driver.exceptions import DatabaseError
from native_store import (
    DATABASE,
    LEADS,
    atomic_json,
    catalog,
    collect,
    disk_guard,
    freshness,
    load,
    lock,
    publish,
)
from pipeline import discard, recover


class PublicationTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.addCleanup(self.temp.cleanup)
        self.root = Path(self.temp.name)

    def entry(self, revision, age=1):
        path = self.root / "revisions" / revision
        path.mkdir(parents=True)
        entry = {
            "revision": revision,
            "database": DATABASE,
            "table": revision,
            "owned": True,
        }
        meta = {
            "model": "hrrr",
            "init_time": datetime.fromtimestamp(
                time.time() - age * 3600, UTC
            ).isoformat(),
            "snapshot_id": revision,
            "leads": LEADS["hrrr"],
        }
        meta.update(
            coverage="conus",
            cells=1638757,
            levels=list(range(1000, 274, -25)),
            variables=[
                "geopotential_height",
                "temperature",
                "wind_u",
                "wind_v",
                "relative_humidity",
                "dew_point_temperature",
            ],
            surface_variables=["temperature_2m", "dew_point_temperature_2m"],
        )
        atomic_json(path / "metadata.json", meta)
        atomic_json(path / "entry.json", entry)
        atomic_json(
            path / "validated.json",
            {**entry, "init_time": meta["init_time"], "snapshot_id": revision},
        )
        return entry

    def test_publish_rollback_and_monotonicity(self):
        a, b, c = (
            self.entry("hrrr_a", 3),
            self.entry("hrrr_b", 2),
            self.entry("hrrr_c", 1),
        )
        publish(self.root, "hrrr", a)
        publish(self.root, "hrrr", b)
        with self.assertRaisesRegex(ValueError, "advance"):
            publish(self.root, "hrrr", a)
        publish(self.root, "hrrr", c)
        state = catalog(self.root)
        self.assertEqual(state["current"]["hrrr"], c)
        self.assertEqual(state["previous"]["hrrr"], b)
        self.assertEqual(state["retired"][0]["revision"], a["revision"])
        publish(self.root, "hrrr", b, rollback=True)
        self.assertEqual(catalog(self.root)["current"]["hrrr"], b)
        self.assertEqual(catalog(self.root)["previous"]["hrrr"], c)

    def test_failed_or_stale_candidate_cannot_replace_active(self):
        a = self.entry("hrrr_a")
        publish(self.root, "hrrr", a)
        before = (self.root / "catalog.json").read_bytes()
        b = self.entry("hrrr_b", 30)
        with self.assertRaisesRegex(ValueError, "too old"):
            publish(self.root, "hrrr", b)
        (self.root / "revisions" / "hrrr_b" / "validated.json").unlink()
        with self.assertRaises(FileNotFoundError):
            publish(self.root, "hrrr", b)
        self.assertEqual((self.root / "catalog.json").read_bytes(), before)

    def test_reader_holds_publication_until_query_finishes(self):
        a, b = self.entry("hrrr_a", 2), self.entry("hrrr_b", 1)
        publish(self.root, "hrrr", a)
        finished = threading.Event()

        def writer():
            publish(self.root, "hrrr", b)
            finished.set()

        with lock(self.root):
            worker = threading.Thread(target=writer)
            worker.start()
            self.assertFalse(finished.wait(0.1))
            self.assertEqual(catalog(self.root)["current"]["hrrr"], a)
        worker.join(2)
        self.assertTrue(finished.is_set())

    def test_recovery_drops_only_unpublished_candidate(self):
        a, b = self.entry("hrrr_a", 2), self.entry("hrrr_b", 1)
        publish(self.root, "hrrr", a)
        for entry in (a, b):
            path = self.root / "revisions" / entry["revision"]
            (path / "lead-000.npy").write_bytes(b"partial")
            (path / "cells.npz").write_bytes(b"grid")
        with patch("pipeline.client") as connection:
            recover(self.root)
            connection.return_value.command.assert_called_once_with(
                f"DROP TABLE IF EXISTS {DATABASE}.hrrr_b SYNC"
            )
        self.assertEqual(catalog(self.root)["current"]["hrrr"], a)
        self.assertFalse((self.root / "revisions" / "hrrr_a" / "lead-000.npy").exists())
        self.assertTrue((self.root / "revisions" / "hrrr_a" / "cells.npz").exists())
        self.assertFalse((self.root / "revisions" / "hrrr_b" / "cells.npz").exists())
        with patch("pipeline.client") as connection:
            discard(self.root, self.root / "revisions" / "hrrr_a")
            connection.assert_not_called()

    def test_gc_keeps_current_and_previous(self):
        entries = [
            self.entry(f"hrrr_{name}", age)
            for name, age in (("a", 3), ("b", 2), ("c", 1))
        ]
        for entry in entries:
            publish(self.root, "hrrr", entry)
        with patch("native_store.client") as connection:
            collect(self.root, grace=0)
            connection.return_value.command.assert_called_once_with(
                f"DROP TABLE IF EXISTS {DATABASE}.hrrr_a SYNC"
            )
        self.assertTrue((self.root / "revisions" / "hrrr_b").exists())
        self.assertTrue((self.root / "revisions" / "hrrr_c").exists())
        self.assertEqual(catalog(self.root)["retired"], [])

    def test_validation_identity_and_disk_admission(self):
        entry = self.entry("hrrr_a")
        path = self.root / "revisions" / "hrrr_a" / "metadata.json"
        meta = json.loads(path.read_text())
        meta["snapshot_id"] = "changed"
        atomic_json(path, meta)
        with self.assertRaisesRegex(ValueError, "does not match"):
            publish(self.root, "hrrr", entry)
        with patch("native_store.shutil.disk_usage") as usage:
            usage.return_value.free = 19 * 1024**3
            with self.assertRaises(OSError):
                disk_guard(self.root)

    def test_changed_source_schema_is_not_published(self):
        entry = self.entry("hrrr_a")
        path = self.root / "revisions" / "hrrr_a" / "metadata.json"
        meta = json.loads(path.read_text())
        meta["cells"] -= 1
        atomic_json(path, meta)
        with self.assertRaisesRegex(ValueError, "schema/coverage"):
            publish(self.root, "hrrr", entry)
        self.assertEqual(catalog(self.root)["current"], {})

    def test_freshness_future_and_remaining_horizon(self):
        meta = {
            "model": "hrrr",
            "init_time": "2026-10-18T00:00:00Z",
            "leads": LEADS["hrrr"],
        }
        now = datetime(2026, 10, 18, tzinfo=UTC).timestamp()
        self.assertTrue(freshness(meta, now + 3600)["available"])
        self.assertFalse(freshness(meta, now - 7200)["available"])
        self.assertFalse(freshness(meta, now + 25 * 3600)["available"])
        meta["leads"] = list(range(19))
        self.assertFalse(freshness(meta, now + 7 * 3600)["available"])


@unittest.skipUnless(
    os.environ.get("TEST_NATIVE_DB") == "1",
    "real local ClickHouse integration is opt-in",
)
class LoaderTests(unittest.TestCase):
    def test_real_insert_validate_and_corruption_gate(self):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            table = "gfs_test_" + uuid.uuid4().hex
            meta = {
                "model": "gfs",
                "coverage": "conus",
                "cells": 2,
                "leads": LEADS["gfs"],
                "levels": [1000, 850],
                "variables": ["temperature"],
                "surface_variables": ["temperature_2m"],
                "init_time": datetime.now(UTC).isoformat(),
                "snapshot_id": "synthetic-test-fixture",
            }
            atomic_json(root / "metadata.json", meta)
            np.savez(root / "cells.npz", cell_id=np.array([7, 19], dtype=np.uint32))
            for lead in meta["leads"]:
                np.save(
                    root / f"lead-{lead:03d}.npy",
                    np.full((2, 2, 1), lead / 7, dtype=np.float32),
                )
                np.save(
                    root / f"surface-{lead:03d}.npy",
                    np.full((2, 1), lead / 3, dtype=np.float32),
                )
                atomic_json(
                    root / f"lead-{lead:03d}.json",
                    {"fields": {"temperature": {"finite": 4, "total": 4}}},
                )
            c = client()
            try:
                result = load(root, root, table)
                self.assertEqual(result["rows"], 282)
                self.assertTrue(result["staged_comparison"])
                with self.assertRaises(DatabaseError):
                    load(root, root, table)  # Must never overwrite an existing table.
                np.save(
                    root / "lead-000.npy", np.full((2, 2, 1), np.nan, dtype=np.float32)
                )
                from native_store import validate

                with self.assertRaises(AssertionError):
                    validate(root, table, staged=True)
                self.assertFalse((root / "validated.json").exists())
                c.command(
                    f"CREATE TABLE {DATABASE}.{table}_clone CLONE AS {DATABASE}.{table}"
                )
                c.command(f"TRUNCATE TABLE {DATABASE}.{table}")
                self.assertEqual(
                    c.query(f"SELECT count() FROM {DATABASE}.{table}_clone").first_row[
                        0
                    ],
                    282,
                )
                c.command(
                    f"CREATE TABLE {DATABASE}.{table}_expired CLONE AS {DATABASE}.{table}_clone"
                )
                current = {
                    "revision": "gfs_current",
                    "database": DATABASE,
                    "table": table + "_clone",
                    "owned": True,
                }
                previous = {
                    "revision": "gfs_previous",
                    "database": DATABASE,
                    "table": table,
                    "owned": True,
                }
                retired = {
                    "revision": "gfs_expired",
                    "database": DATABASE,
                    "table": table + "_expired",
                    "owned": True,
                    "retired_at": 0,
                }
                atomic_json(
                    root / "catalog.json",
                    {
                        "version": 1,
                        "generation": 1,
                        "current": {"gfs": current},
                        "previous": {"gfs": previous},
                        "retired": [retired],
                    },
                )
                collect(root, grace=0)
                self.assertEqual(
                    c.command(f"EXISTS TABLE {DATABASE}.{table}_expired"), 0
                )
                self.assertEqual(c.command(f"EXISTS TABLE {DATABASE}.{table}"), 1)
                self.assertEqual(
                    c.query(f"SELECT count() FROM {DATABASE}.{table}_clone").first_row[
                        0
                    ],
                    282,
                )
            finally:
                c.command(f"DROP TABLE IF EXISTS {DATABASE}.{table}_expired SYNC")
                c.command(f"DROP TABLE IF EXISTS {DATABASE}.{table}_clone SYNC")
                c.command(f"DROP TABLE IF EXISTS {DATABASE}.{table} SYNC")
                c.close()


if __name__ == "__main__":
    unittest.main()
