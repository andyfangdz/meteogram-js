# Incremental ingestion evidence

Small, inspectable records from the 2026-10-18 home-host rollout. Large source
maps, staged weather, dictionaries and ClickHouse data remain outside Git.

- `availability-hrrr-short.*`: metadata-only discovery of the hourly HRRR source.
- `preview-update-*.time`, `preview-http.json`: two separate live GFS polls and
  a four-model HTTP/database comparison while GFS was partial.
- `rrfs-backup-*`: full 85-hour acquisition, loading, validation and historical
  adoption needed to establish two actual complete RRFS backups before cutover.
  The first load attempted a missing working directory; it was corrected before
  the recorded successful load. No partial table was published.
- `initial-*`, `health-after-install.json`: catalog, availability and polling
  state around the first incremental installation. Snapshots are taken at
  different times and should not be treated as a single atomic observation.
- `archive-http.json`: all eight complete selectors plus HRRR's long selector,
  and each model's run inventory while partial runs were current.
- `audit-{gfs,hrrr}.json`: independent full audits of published partial manifests.
- `first-rollout-failures.json`, `live-observations.jsonl`: initial failures under
  the old 6 GiB database budget. These precede the user's approved increase.
- `live-12gib-*`, `installed-12gib-http.json`: the successful 12 GiB live window,
  with one-second database/host memory observations, periodic HTTP health and
  complete poll reports. Sampled memory is not an instantaneous peak guarantee.
- `cleanup.json`: only owned bootstrap/preview tables were dropped; the retained
  production clone, all backup tables and benchmark tables were protected.

The accompanying plan distinguishes complete historical backups from the
building current forecasts. A successful per-hour publication is not a completed
new forecast run. Full value/index/shape/key validation plus sampled exact
source comparisons is not a second bitwise comparison of every source value.

Final installation adds `candidate.sha256`, `source.sha256`, the Cargo check logs,
`final-http.json`, `final-status.json`, `installed-server-limit.json` and
`restart-verification.json`. The latter checks immutable books and unique
published-hour tiles across restart, complete archive preservation and the
successful first resumed poll. `stopped-*` records the drained shutdown state.
