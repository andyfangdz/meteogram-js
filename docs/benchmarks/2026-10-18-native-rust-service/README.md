# Full Rust service migration evidence

> Historical benchmark evidence. Results and implementation details describe the
> recorded version; use the [current native service guide](../../../native/README.md)
> for supported operation.

See the [migration report](../../archive/2026-10-18-native-rust-service.md) for
interpretation and limits. All files describe actual host checks; large source
arrays, caches, database files and executables are excluded.

- `summary.json`, `full-cycle.json`, `full-cycle-events.jsonl`: completed fresh
  pinned acquisition/loading/publication into an isolated catalog.
- `*-source.json`, `*-source-codecs.json`: source payload counts, finite values
  and actual Zarr codec/dimension metadata for all three models.
- `*-load.json`, `*-validation.json`: bounded loader timings and full-table /
  exact staged-sample gates. Logical and physical row counts are separate.
- `exact-store-comparison.json`: all grid IDs/coordinates plus 100 complete
  profiles per model against the retained reference store.
- `candidate-api-comparison.json`: 27 complete candidate HTTP/DB/reference
  profiles, input/boundary checks and gzip.
- `rollback-*.json`, `restore-*.json`: actual mixed-format rollback checks;
  restored weather hashes match the initial candidate.
- `http-during-ingest.json`: 1,023 successful fixed-point requests against the
  existing Python API during the isolated build. The serving catalog did not
  change; a 3.994-second GFS response was observed despite zero errors.
- `live-full-validation.json`, `final-live-api.json`: full retained-current
  database checks and live Rust API checks around restart recovery.
- `before-db-restart.json`, `after-db-restart.json`, `service-final.json`: actual
  Rust supervisor/database/API status; the catalog and schedule were preserved.
- `cleanup-plan.json`, `experimental-catalog-before-cleanup.json`: explicit
  experimental ownership, protected serving tables, and descriptors saved before
  removing experimental data. Experimental catalogs now have no serving entries.
- `final-durable-source-*.json`: a final-binary fresh AIFS lead-0 acquisition,
  synced checkpoint, insertion and staged-value verification.
- `tests.txt`, `clippy.txt`, `rustc-version.txt`, `build-sha256.txt`: 22 passing
  tests (three real database tests), static checks and build identity.
- `initial-source-failure.json`, `initial-api-compatibility.json`: preserved
  initial S3 anonymous-option failure and pre-cutover HTTP compatibility checks.

The user unit was statically verified; its transcript contains an unrelated
installed spice-vdagent unit warning. The user manager bus was unavailable in
this execution environment, so runtime supervision was exercised directly.
Reboot activation was not tested. No application weather-provider switch or
public endpoint was deployed.
