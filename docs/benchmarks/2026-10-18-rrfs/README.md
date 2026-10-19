# Complete RRFS ingestion evidence — 2026-10-18

> Historical benchmark evidence. Results and implementation details describe the
> recorded version; use the [current native service guide](../../../native/README.md)
> for supported operation.

See the [results and operating notes](../../archive/2026-10-18-rrfs-ingestion.md).
This is the complete retained 85-lead, 30-level, six-pressure-field/two-surface-field
selection over 1,638,757 CONUS cells. It does not claim all variables/levels in
the upstream product or a deployed Next.js weather-provider adapter.

- `summary.json`, `metadata.json`, `source-leads.json`, `source-arrays.json`:
  exact pinned selection, all completed lead payload counts and stage cardinality.
- `source-preflight-{1m,8m}.json`, `grid-comparison.json`: complete source
  reference availability and manifest-cache/grid observations.
- `dictionary.json`, `load-timings.json`, `load.log`, `validate.json`,
  `merged-clone-audit.json`, `published-validation.json`: the complete load,
  palette/index/shape/key proofs and staged sample comparisons.
- `*-resource.txt`: command wall clocks and peak client RSS. The first 14-lead
  source command deliberately exits 1 before the next lead; its completed
  checkpoints are reused. Failed broad-audit logs are retained separately.
- `final-audit-query-memory.json`: ClickHouse query-log peaks for the final
  per-column predicates; includes actual read rows and cache scope.
- `load-server-resources-v2.json`: one-second memory samples beginning partway
  through loading, including earlier failed audits, not a full load-only peak.
- `before-api.json`, `preinstall-api.json`, `postinstall-api.json`,
  `*-comparison.json`, `post-cleanup-health.json`: exact weather-row hashes and
  HTTP/model availability checks before/after publication.
- `before-tables.json`, `after-tables.json`, `cleanup.json`,
  `before-catalog.json`, `after-catalog.json`: ownership and preservation of the
  12 pre-existing tables, independent published RRFS revision and cleanup.
- `observer.json`, `restart-health.json`, `second-restart-health.json`: fixed-point
  existing-model requests and the two planned health gaps (6.02 and 1.55 seconds).
- `four-model-comparison-smoke.json`: same-catalog, one-profile-per-model
  diagnostic compatibility test; not independent source-equality evidence.
- `tests.log`, `clippy.log`, `format.log`, `build.log`, `source.sha256`,
  `candidate.sha256`, `binary-installed.json`: verification and final build identity.
  `first-binary-installed.json` / `ingestion-audit-candidate.sha256` identify the
  earlier audit/first installed binary; the final change fixes only the
  comparison diagnostic's model-set gate.

`manifest.json` records SHA-256 and size for each evidence file. Large codebooks,
coordinates, weather staging and database parts are intentionally excluded.
The source/standalone measurements precede removal of temporary weather staging;
future scheduled refreshes are configured, not measured by this first ingestion.
