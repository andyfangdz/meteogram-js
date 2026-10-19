# Full GFS dictionary evidence

Interpretation and exact scope: [report](../../plans/2026-10-18-gfs-dictionary.md).
The final configuration matrix contains nine complete fresh-table staged loads,
three repeats each of raw bits, dictionary profiles and 256-cell tiles. All use
four workers and the same frozen final binary. Acquisition/preparation and
additional full reconstruction audits are outside the load clock.

## Evidence map

- `summary.json`: pinned source/axes/fields, preparation, every final run,
  storage including required codebooks, independent audits and query results.
- `source.json`: immutable source-table clone and full 06Z source metadata.
  Cloning freezes preparation input; timed loads always insert into new tables.
- `prepared-*-metadata.json`: original raw/dictionary descriptors before loading.
- `*-preparation.json`, `*-prepare-process.json`: full preparation scope, encoded
  sizes, process times and RSS. The dictionary encoder peaks at 310 MiB; the
  separate preceding raw exporter peaks at 1.78 GiB.
- `dictionary-cardinalities.json`, `cardinality-comparison.json`: full codebook
  counts exactly match the earlier complete-source diversity diagnostic.
- `legacy-hrrr-codebook.json`: successful reading of the existing full HRRR book.
- `runs/*/process.json`: entire CLI elapsed time, CPU, RSS and filesystem counters.
- `runs/*/load.json`, `validation.json`: timed loader work and completion gates.
- `runs/*/range-summary.json`: actual successful query IDs,
  disjoint expected/read row counts, original range headers and resource counters.
  Final repeats retain `range-queries.jsonl` with the original observations.
  SQL log strings can be truncated; full predicate generation is in Rust source.
- `runs/*/parts.jsonl`: active rows/parts and database bytes before later merging.
- `runs/*/input-cache.json`: Linux discard hints for owned weather files and book.
- `runs/*-r3/independent-audit.json`: additional whole-table `validate --staged`.
- `runs/*-r3/exact.json`: every native ID/coordinate and 100 complete forecasts
  per model against the immutable reference, including unchanged-model compatibility.
- `runs/*-r3/http.json`: 27 complete HTTP comparisons, bad/outside requests, gzip.
- `runs/*-r3/queries.json`: 300 warmup + 300 measured complete GFS profile queries,
  one client; codebook resident, local database/client decoding only.
- `roundtrip-comparison.json`, dictionary `*-r3/full-roundtrip.sha256`: complete
  reconstruction of both dictionary layouts, each matching all 565 original raw
  weather/index/coordinate files. Every one of 638,598,870 Float32 values is covered.
  Payloads are not committed; hashes were checked before their cleanup.
- `runs/*-r3/roundtrip-process.json`: resource costs of those separate audits.
- `point-query-bytes.jsonl`: ClickHouse result/read byte counters for 738 point
  queries each before/after projection. The database still reads whole tile data.
- `before-projection/`: earlier query results and summary using the same full
  forecast/codebook/storage layouts, before optimizing response projection and
  dictionary decoding. Its earlier nine loads are not the final timing matrix.
- Final source/binary hashes, `tests-v2.log`, `clippy-v2.log`, `format-v2.log` and
  `build-v2.log`: frozen final source/build and 42 passing tests, including DB tests.
- `unchanged-encoder.json`: preparation/dictionary/CLI sources match between
  phases; only point-reader code and its corruption test changed.
- `evidence-validation.json` and its script: parsed evidence, checked full scope,
  medians/arithmetic, reconstruction proofs, preservation and local document links.
  `bundle.sha256` fingerprints the other files in this directory.
- Main state snapshots, protected row counts, `service-preservation.json`, final
  HTTP/health and observer: all twelve original tables and the installed binary,
  catalog/schedule preserved. The observer recorded 611 successes/model, no errors,
  at one NY point/model; it is not a general availability guarantee.
- `cleanup-plan.json`, `cleanup.json`: the nine earlier tables, nine final tables
  and single preparation clone were removed, with only owned weather staging.
  Immutable identical book copies were shared with hardlinks after verification.
- `host.json`: unchanged host/database memory limits.

## Commands and retained artifacts

The scripts record this host's exact isolated workflow. Their external root is
`~/.local/share/meteogram-gfs-dictionary`. For repetition, use a fresh root,
unique tables and complete pinned inputs; do not apply cleanup to other owners.

`build-and-prepare.sh` records the initial encoder build and complete raw/mapwise
preparation. `build-v2.sh` builds the final point-reader optimization; the encoder
and staged inputs are unchanged. `run-loads.sh` resets original descriptors,
advises only immutable input files out of cache and runs the nine final loads.
Required books are hardlinked read-only after each clock, avoiding large result
copies between measurements. `verify.sh` performs independent/reference/HTTP
checks, two complete raw reconstructions, then separate warm query measurements.
`check-roundtrip.mjs` checks all files and `summarize.mjs` checks/consolidates scope
and successful disjoint ranges. `finish.mjs` guards the main catalog and explicit
ownership before cleaning up, then rechecks the protected tables and service.

Codebooks, coordinates, descriptors and the two frozen binaries remain outside
Git. Timed tables and weather payloads have been removed. The final 169 MB
codebook is required data, not disposable weather staging. No binary was installed,
no service restarted and no automatic GFS dictionary acquisition was enabled.
New GFS books above the previous per-field reader cap require upgraded readers
before any future publication. See `encode-stage` in the native README.
