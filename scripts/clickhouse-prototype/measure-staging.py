"""Measure lossless compressed checkpoints using completed real source samples."""

import argparse
import json
import time
from pathlib import Path

import numpy as np
from numcodecs import Zstd


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--data", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument(
        "--layouts", default="interleaved,by_field,by_field_4096_cell_blocks"
    )
    args = parser.parse_args()
    codec = Zstd(level=1)
    reports = []
    for checkpoint in sorted(args.data.glob("lead-*.json")):
        lead = int(checkpoint.stem.split("-")[1])
        a = np.load(args.data / f"lead-{lead:03d}.npy")
        surface = np.load(args.data / f"surface-{lead:03d}.npy")
        for layout in args.layouts.split(","):
            if layout not in ("interleaved", "by_field", "by_field_4096_cell_blocks"):
                raise ValueError(f"Unknown staging layout: {layout}")
            arrays = [a, surface]
            if layout != "interleaved":
                batch = 4096 if layout.endswith("blocks") else len(a)
                arrays = []
                for start in range(0, len(a), batch):
                    arrays.extend(
                        np.ascontiguousarray(a[start : start + batch, :, i])
                        for i in range(a.shape[-1])
                    )
                    arrays.append(np.ascontiguousarray(surface[start : start + batch]))
            encoded_size = 0
            encode_seconds = decode_seconds = 0.0
            for array in arrays:
                start = time.perf_counter()
                encoded = codec.encode(array)
                encode_seconds += time.perf_counter() - start
                encoded_size += len(encoded)
                start = time.perf_counter()
                decoded = codec.decode(encoded)
                decode_seconds += time.perf_counter() - start
                np.testing.assert_array_equal(
                    np.frombuffer(decoded, dtype=np.float32).reshape(array.shape), array
                )
            reports.append(
                {
                    "lead": lead,
                    "layout": layout,
                    "raw_bytes": a.nbytes + surface.nbytes,
                    "compressed_bytes": encoded_size,
                    "encode_seconds": encode_seconds,
                    "decode_seconds": decode_seconds,
                }
            )
    args.output.write_text(json.dumps(reports, indent=2) + "\n")
    print(json.dumps(reports, indent=2))


if __name__ == "__main__":
    main()
