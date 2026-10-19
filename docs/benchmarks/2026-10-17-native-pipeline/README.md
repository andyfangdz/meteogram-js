# Home native pipeline deployment evidence

> Historical benchmark evidence. Results and implementation details describe the
> recorded version; use the [current native service guide](../../../native/README.md)
> for supported operation.

See the [deployment report](../../archive/2026-10-17-native-pipeline-deployment.md)
for results, operating commands and limitations.

- `summary.json`: accepted source identities/volumes, retained table sizes,
  observed disk measurements and next refresh time.
- `current-*-metadata.json`, `current-*-validated.json`: real 18Z source selections
  and full-table validation markers, including exact sampled staging comparisons.
- `previous-*-metadata.json`, `previous-*-validated.json`: independent 12Z snapshots
  retained for rollback. Database validation was repeated after cloning.
- `live-update-verification.json`, `http-transitions.json`: 2,817 error-free HTTP
  requests, all three initialization transitions and the largest latency outliers.
  Full observations remain at the recorded local path; the HTTP tail reached
  9.94 seconds and its cause outside measured SQL execution is unresolved.
- `rollback-verification.json`: all three real rollbacks and exact response
  restoration hashes.
- `restart-verification.json`: successful automatic recovery after isolating
  child process groups, preserving retained rows, catalog and schedule.
- `final-service-verification.json`: final HTTP/data/TTL checks after restart and
  cleanup, with the service running.
- `supervisor-fault-check.json`: initial supervisor cleanup after an injected
  failure in its disposable heartbeat-output path.
- `catalog.json`, `events.jsonl`, `last-update.json`, `schedule.json`: publication,
  rejected candidate, isolation/rollback history and operational state at capture.
- `benchmark-cleanup.json`: exact scope and logical bytes of reclaimed historical
  raw arrays and source cache. Tables and small evidence were preserved.
- `deployment-telemetry.json`: partial five-second disk/part samples; sampling
  began partway through HRRR loading, so peaks are not complete-run guarantees.
- `pipeline-tests.txt`, `serve-tests.txt`, `http-contract-checks.json`: focused
  tests, real tiny-fixture DB checks and live HTTP error contracts.
- `systemd-startup-check.txt`: filtered startup graph evidence. Live user-manager
  activation and reboot were not tested.

The database fixture used by the test suite is synthetic and explicitly labeled;
all forecast transition, source and retained-volume results use actual data.
Logical sizes of cloned tables can count shared files twice. This bundle contains
no GRIB payloads, raw forecast arrays, database data, credentials or binaries.
