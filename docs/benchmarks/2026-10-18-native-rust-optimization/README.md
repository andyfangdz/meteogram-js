# Rust/profile ingestion evidence

See the [measured report](../../plans/2026-10-18-native-rust-optimization.md).
The pinned full cycle took 959.820 seconds. It retained 84,579,313 logical
cell/lead records in 1,688,527 physical cell rows, occupying 25.935 GB.

- `complete.json`, `summary.json`, `events.jsonl`: complete build and resource measurements.
- `*-validated.json`, `*-load-timings.json`: source/storage identity, gates and overlapping phase work.
- `model-benchmark.json`, `bits-benchmark.json`, `rust-benchmark.json`: layout/codec/write-path screening.
- `screening-method.json`: timing boundaries and prototype implementation caveats.
- `rust-build.json`, `rust-pack-benchmark.json`: compiler/source identity and emitted NEON proof/microbenchmarks.
- `rust-gated-sample.json`: integrated small sample during concurrent acquisition; not an isolated speed comparison.
- `http-summary.json`: 1,350 successful original-service requests during the isolated build.
- `pre-restart-http-verification.json`, `candidate-http-verification.json`: exact HTTP comparisons before/after database reopening.
- `mixed-layout-rollback.json`: new-profile to legacy and restore checks for each model.
- `query-comparison.json`: final uninterrupted warm single/four-client comparison after recovery.
- `reader-reload-verification.json`, `final-service-verification.json`: automatic restart, reader deployment, retained data and schedule.
- `cleanup.json`: only owned experiment tables and sample arrays removed; serving/borrowed tables preserved.
- `*-tests.txt`: 23 passing tests.

The source cycle and snapshots are identical to the previous optimization.
The serving catalog stayed unchanged during acquisition/build, so availability
results do not test publication-lock tails. Phase work overlaps; use total wall
time for full comparisons. Physical UInt32 arrays encode exact Float32 bits.
Auxiliary small DB checks ran during early acquisition and are included in
server-event/resource counters. Experimental tables and raw/sample staging no
longer exist; table names in these files are historical evidence, not serving
revisions. Borrowed old-format tables used in rollback tests were not deleted.
