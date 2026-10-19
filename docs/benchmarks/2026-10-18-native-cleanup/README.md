# Native path cleanup verification — 2026-10-18

The native service accepts the per-hour UInt16 dictionary/tile format used by
HRRR, GFS, AIFS and RRFS. Whole-run loading, raw/profile storage readers, NumPy
imports and conversion commands were removed. The Open-Meteo fallback remains
part of the application. Current operating instructions are in the
[native service guide](../../../native/README.md).

Validation covers 48 Rust tests, including the loopback ClickHouse integration
tests, 109 frontend tests, TypeScript, lint, Rust formatting and Clippy. Candidate
API checks compare 36 profiles with database decoding across all four models,
exercise invalid and outside-grid inputs, and verify gzip. Archive checks cover
all 13 current, complete, previous-complete and HRRR long-run selections.

The sole retained whole-run archive, RRFS initialized at 2026-10-18 12:00 UTC,
was converted separately before activating the stricter reader. It retains all
1,638,757 cells, 85 forecast hours, 30 pressure levels, six pressure fields and
two surface fields: 25,351,570,790 weather values. Verification includes:

- Identical canonical hashes of every packed index and original ordered cell ID.
- Bit-identical Float32 palettes, covering all 150,185,270 palette entries.
- Full hourly layout/index audits and 100 identical complete point forecasts.
- Unchanged current forecast selections when the replacement archive published.

The combined index and palette checks establish exact weather-value identity;
no quantization or spatial/temporal thinning was introduced. The one-time
conversion ran outside the repository and is not an installed command.

[`verification.json`](verification.json) records the hashes and completed
verification results. Historical performance measurements remain in their
original dated records; this cleanup does not introduce new loader benchmarks.
