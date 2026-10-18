"""Regression checks for native edge-cell and retained-region resolution."""

import unittest
from types import SimpleNamespace

import numpy as np
from serve import resolve


class ResolveTests(unittest.TestCase):
    def model(self):
        return {
            "metadata": {
                "grid": {"kind": "projected", "x": [0.0, 1.0], "y": [0.0, 1.0]}
            },
            "projection": SimpleNamespace(transform=lambda lon, lat: (lon, lat)),
            "cells": {"cell_id": np.arange(4)},
        }

    def test_projection_roundtrip_noise_accepts_edge_cell(self):
        self.assertEqual(resolve(self.model(), 1.0 + 1e-9, 1.0), (3, 3))
        self.assertEqual(resolve(self.model(), -1e-9, 0.0), (0, 0))

    def test_actual_outside_native_domain_is_rejected(self):
        with self.assertRaises(LookupError):
            resolve(self.model(), 1.0 + 1e-4, 1.0)

    def test_conus_boundary_checked_before_nearest_cell(self):
        model = self.model()
        model["metadata"]["coverage"] = "conus"
        model["projection"] = SimpleNamespace(transform=lambda lon, lat: (0.0, 0.0))
        self.assertEqual(resolve(model, 24.0, -100.0), (0, 0))
        with self.assertRaisesRegex(LookupError, "Outside the retained region"):
            resolve(model, 24.0 - 1e-8, -100.0)
        with self.assertRaisesRegex(LookupError, "Outside the retained region"):
            resolve(model, 40.0, -125.0 - 1e-8)


if __name__ == "__main__":
    unittest.main()
