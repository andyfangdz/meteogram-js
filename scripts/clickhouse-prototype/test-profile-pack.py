"""SIMD transpose tails, IEEE bit preservation and finite-value rejection."""

import unittest

import numpy as np
from profile_pack import backend, library, pack


class PackingTests(unittest.TestCase):
    def test_axes_and_all_tail_shapes_are_bit_exact(self):
        rng = np.random.default_rng(19)
        modes = (
            ["numpy"]
            + (["scalar"] if library() else [])
            + (["neon"] if backend() == "neon" else [])
        )
        for C, L, F, T in [
            (3, 2, 6, 1),
            (7, 30, 6, 49),
            (5, 8, 5, 31),
            (3, 1, 2, 141),
            (1, 5, 6, 7),
            (4, 4, 6, 4),
        ]:
            source = [
                rng.uniform(-100, 100, (C, L, F)).astype("float32") for _ in range(T)
            ]
            source[0].flat[0] = -0.0
            source[0].flat[1] = np.nextafter(np.float32(0), np.float32(1))
            expected = np.ascontiguousarray(
                np.stack(source, axis=1).transpose(3, 0, 2, 1)
            )
            for mode in modes:
                with self.subTest(shape=(C, L, F, T), mode=mode):
                    np.testing.assert_array_equal(
                        pack(source, mode).view("uint32"), expected.view("uint32")
                    )

    def test_rejects_nonfinite_values_in_vector_and_scalar_tails(self):
        modes = (
            ["numpy"]
            + (["scalar"] if library() else [])
            + (["neon"] if backend() == "neon" else [])
        )
        for value in [np.nan, np.inf, -np.inf]:
            for t, z in [(0, 0), (3, 3), (4, 4)]:
                arrays = [np.zeros((2, 5, 6), dtype="float32") for _ in range(5)]
                arrays[t][1, z, 5] = value
                for mode in modes:
                    with (
                        self.subTest(value=value, t=t, z=z, mode=mode),
                        self.assertRaisesRegex(ValueError, "Non-finite"),
                    ):
                        pack(arrays, mode)

    def test_rejects_bad_shapes_types_and_noncontiguous_views(self):
        for arrays in [
            [],
            [np.zeros((2, 3, 6), dtype="float64")],
            [np.zeros((2, 3, 6), dtype="float32")[:, ::2]],
            [
                np.zeros((2, 3, 6), dtype="float32"),
                np.zeros((1, 3, 6), dtype="float32"),
            ],
        ]:
            with self.assertRaises(ValueError):
                pack(arrays)


if __name__ == "__main__":
    unittest.main()
