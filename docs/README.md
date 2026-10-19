# Documentation

Current operation and architecture:

- [Application setup and provider behavior](../README.md)
- [Native service setup, storage, ingestion, API and verification](../native/README.md)
- [Agent guide and code invariants](../AGENTS.md)

The native service is deployed for HRRR, GFS, AIFS and RRFS. All four use the
per-hour UInt16 dictionary/tile format. The site includes native HRRR cloud
layers and initialization-aware overlays, with Open-Meteo fallback.

## Validation records

- [Native path cleanup and retained RRFS archive conversion](benchmarks/2026-10-18-native-cleanup/README.md)

## Historical native investigations and rollout records

[`archive/`](archive/) contains dated designs, measurements and rollout evidence.
Statements such as “not deployed,” “experimental” or “defaults unchanged” refer
to the version and stage recorded in that document. Their commands can reference
retired tooling; they are not current operating instructions. Benchmark evidence
under [`benchmarks/`](benchmarks/) preserves the original measurements and source
identities, including failed and rejected approaches.

- [Dynamical migration investigation](archive/2026-10-14-dynamical-migration.md)
- [Near-zero-cost Dynamical ingestion and serving](archive/2026-10-16-dynamical-cost-design.md)
- [Full-volume retry: full CONUS geography, incomplete forecast horizon](archive/2026-10-17-clickhouse-full-volume-retry.md)
- [Full-resident CONUS benchmark on the home ARM host](archive/2026-10-17-clickhouse-home-resident.md)
- [ClickHouse prototype: measured results and home-server estimates](archive/2026-10-17-clickhouse-prototype-results.md)
- [Arbitrary US locations from a home forecast server](archive/2026-10-17-dynamical-arbitrary-locations.md)
- [Native ingestion optimization — measured on the home server](archive/2026-10-17-native-ingestion-optimization.md)
- [Native forecast pipeline on the home server](archive/2026-10-17-native-pipeline-deployment.md)
- [Bounded full GFS dictionaries — 2026-10-18](archive/2026-10-18-gfs-dictionary.md)
- [Full HRRR loading and native compressed staging](archive/2026-10-18-hrrr-load-optimization.md)
- [Native HRRR clouds and initialization overlays](archive/2026-10-18-hrrr-native-clouds-and-runs.md)
- [Full HRRR loading under one minute](archive/2026-10-18-hrrr-one-minute-load.md)
- [Incremental forecast ingestion and complete-run retention](archive/2026-10-18-incremental-ingestion.md)
- [Native ingestion: packing locality, source preflight and bounded cache](archive/2026-10-18-native-ingestion-optimization.md)
- [Rust, SIMD and profile storage — measured home-server optimization](archive/2026-10-18-native-rust-optimization.md)
- [Native forecast service: full Rust migration](archive/2026-10-18-native-rust-service.md)
- [Native forecast site integration — 2026-10-18](archive/2026-10-18-native-site-integration.md)
- [Full GFS and AIFS loader benchmarks — 2026-10-18](archive/2026-10-18-other-model-load-benchmarks.md)
- [RRFS ingestion and uniform codebook allowance](archive/2026-10-18-rrfs-ingestion.md)
- [Home Linux benchmark handoff (archived)](archive/dynamical-home-agent-handoff.md)

## Other designs

- [Humidity isolines design](plans/2026-01-18-humidity-isolines-design.md)
