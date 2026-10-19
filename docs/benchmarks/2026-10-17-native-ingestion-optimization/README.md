# Ingestion optimization evidence

See the [measured report](../../plans/2026-10-17-native-ingestion-optimization.md).
The full pinned 18Z cycle completed in 1,210.524 seconds and loaded 84,579,313 rows.

- `baseline-events.jsonl`: earlier live-update phase timestamps, including rejected HRRR candidate.
- `events.jsonl`, `complete.json`, `summary.json`: optimized run timing, counts, source payload and resources.
- `*-load-timings.json`, `*-validated.json`: per-model phase timing and validation markers.
- `sample-comparison.json`, `optimized*-timings.json`: four sample loader comparisons.
- `baseline-query-profile.json`: earlier HRRR query-log profiling; this does not include all merge/client time.
- `http-summary.json`, `http-slowest.json`: availability of the original API during the isolated build.
- `candidate-http-verification.json`: 42 exact HTTP profile comparisons at cities/random/edge cells.
- `query-comparison.json`: explicit warm-up and one/four-client comparisons of both full layouts.
- `*-tests.txt`: 18 passing tests across three suites.
- `cleanup.json`, `final-service-verification.json`: temporary data removed, serving catalog/schedule preserved.

The run used the same source initializations/snapshots as the earlier native
pipeline deployment. Metadata selection was pinned. The serving catalog was not
changed during this benchmark; availability results do not measure publication
lock contention. Resource sampling began partway through acquisition but covered
all loading. Updater process I/O may include reaped source children. Raw staging
and source caches were removed by the pipeline after each successful publication;
temporary candidate tables were removed after all comparisons. Do not interpret
historical table names in these files as currently installed serving revisions.
