# Historical ClickHouse investigation tools

The active native forecast service has moved to [`native/`](../../native/README.md).
It is one Rust/Cargo application for acquisition, loading, queries, publication,
retention, supervision and verification. The Python investigation scripts and
requirements were retired after the full Rust migration.

Their historical implementations remain in commit `5f4ae08` and its predecessors.
Use that revision to reproduce the old script-specific experiments. Large source
caches, staged arrays and database files remain outside the repository.

Measured reports and evidence remain in [`docs/plans/`](../../docs/plans/) and
[`docs/benchmarks/`](../../docs/benchmarks/). The current workflow and migration
checks are documented in
[the Rust service report](../../docs/plans/2026-10-18-native-rust-service.md).
