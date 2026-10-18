"""Concurrency gates: overlap, disk fallback, failed preparation and shutdown."""

import json
import tempfile
import threading
import unittest
from contextlib import ExitStack
from pathlib import Path
from unittest.mock import patch

from pipeline import prepare, update


class SchedulingTests(unittest.TestCase):
    def exercise(
        self, *, overlap=True, fetch_failure=False, load_failure=False, interrupt=False
    ):
        with tempfile.TemporaryDirectory() as temporary, ExitStack() as stack:
            root = Path(temporary)
            next_started = threading.Event()
            next_finished = threading.Event()
            load_started = threading.Event()
            calls = []

            def prepare(root, model, offsets, cancelled):
                path = root / model
                path.mkdir(exist_ok=True)
                if model == "gfs":
                    next_started.set()
                    if interrupt:
                        self.assertTrue(cancelled.wait(2))
                        next_finished.set()
                        raise InterruptedError("cancelled")
                    if overlap:
                        self.assertTrue(load_started.wait(2))
                    next_finished.set()
                    if fetch_failure:
                        raise RuntimeError("source unavailable")
                return path, {"table": model}, {"init_time": "2026-10-18T00:00:00"}

            def load(root, path, table):
                if table == "hrrr":
                    load_started.set()
                    if overlap:
                        self.assertTrue(next_started.wait(2))
                    else:
                        self.assertFalse(next_started.is_set())
                    if interrupt:
                        raise KeyboardInterrupt()
                    if load_failure:
                        raise ValueError("validation failed")
                calls.append(table)

            def guard(root, extra):
                if not overlap:
                    raise OSError("insufficient combined budget")

            for name in ("collect", "recover", "clean_workspace", "discard"):
                stack.enter_context(patch("pipeline." + name))
            stack.enter_context(patch("pipeline.prepare", side_effect=prepare))
            stack.enter_context(patch("pipeline.load", side_effect=load))
            stack.enter_context(patch("pipeline.disk_guard", side_effect=guard))
            published = stack.enter_context(
                patch("pipeline.publish", return_value={"generation": 1})
            )
            if interrupt:
                with self.assertRaises(KeyboardInterrupt):
                    update(root, ["hrrr", "gfs"], [1, 2])
                self.assertTrue(next_finished.is_set())
                published.assert_not_called()
                return
            result = update(root, ["hrrr", "gfs", "aifs"], [1, 2])
            self.assertEqual(result, int(fetch_failure or load_failure))
            self.assertEqual(
                calls,
                ["gfs", "aifs"]
                if load_failure
                else ["hrrr", "aifs"]
                if fetch_failure
                else ["hrrr", "gfs", "aifs"],
            )
            self.assertEqual(published.call_count, len(calls))
            summary = json.loads((root / "last-update.json").read_text())
            self.assertEqual(
                len(summary["failures"]), int(fetch_failure or load_failure)
            )

    def test_fetch_overlaps_load_with_serial_publication(self):
        self.exercise()

    def test_disk_budget_defers_prefetch(self):
        self.exercise(overlap=False)

    def test_failed_prefetch_does_not_block_other_models(self):
        self.exercise(fetch_failure=True)

    def test_interrupt_cancels_and_joins_prefetch(self):
        self.exercise(interrupt=True)

    def test_failed_validation_does_not_publish_or_discard_next_model(self):
        self.exercise(load_failure=True)

    def test_partial_source_values_fall_back_before_loading(self):
        with tempfile.TemporaryDirectory() as temporary, ExitStack() as stack:
            root = Path(temporary)

            def fetch(root, path, model, offset, metadata_only=False, cancelled=None):
                if metadata_only:
                    (path / "metadata.json").write_text(
                        json.dumps(
                            {
                                "model": model,
                                "init_time": "2026-10-18T00:00:00",
                                "leads": [0],
                            }
                        )
                    )
                else:
                    (path / "lead-000.json").write_text(
                        json.dumps(
                            {
                                "fields": {
                                    "temperature": {
                                        "finite": 1 if offset == 1 else 2,
                                        "total": 2,
                                    }
                                },
                            }
                        )
                    )

            stack.enter_context(patch("pipeline.run_fetch", side_effect=fetch))
            stack.enter_context(patch("pipeline.check_metadata"))
            stack.enter_context(patch("pipeline.disk_guard"))
            stack.enter_context(
                patch("pipeline.freshness", return_value={"available": True})
            )
            discard = stack.enter_context(patch("pipeline.discard"))
            result = prepare(root, "hrrr", [1, 2], threading.Event())
            self.assertIsNotNone(result)
            discard.assert_called_once()
            self.assertNotEqual(discard.call_args.args[1], result[0])


if __name__ == "__main__":
    unittest.main()
