# Incremental forecast ingestion and complete-run retention

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

The native service now ingests independently available forecast hours, instead
of waiting for an entire initialization. The default API serves the newest run
as soon as one hour passes validation. It separately protects the two latest
complete runs for every model and the latest complete 48-hour HRRR run, counting
overlap once. A partial initialization never consumes a complete-backup slot.
The Next.js provider remains Open-Meteo.

## Source discovery and publication

The supervisor starts a persistent `watch` child, polling every 60 seconds.
Each poll opens fresh immutable Icechunk sessions while reusing bounded
repository manifest caches. Discovery checks all required pressure and surface
map references for each hour without downloading weather maps. Only an hour
whose complete required schema is referenced can be acquired; missing hours do
not block other hours. Acquisition still verifies actual payloads and codecs.

HRRR now includes both Dynamical products: hourly 18-hour forecasts (19 leads)
and 48-hour forecasts (49 leads) at 00/06/12/18 UTC. Those six-hour initializations
must satisfy the long schema before they count as complete. GFS retains 141
leads, AIFS 31 and RRFS 85. Retained geography, pressure levels, native fields,
units and wind frames are unchanged.

Every new hour is extracted over the full retained CONUS grid, with exact
Float32 dictionaries, into private checksummed staging. A private 256-cell-tile
ClickHouse table passes complete value/index/shape/key checks and ten complete
bit-exact staged profile comparisons. Its partitions attach to the shared run
table, followed by another complete check and staged comparisons. Only then is
an immutable catalog manifest published atomically. An API query selects only
that manifest's validated hours, so partitions attached before a crash cannot
become visible accidentally.

`lead_partition_tiles_v1` uses one table per model, product and initialization,
partitioned by `(lead_hour, intDiv(tile_id,128))`. Each row contains 256 native
cells for one hour. Point requests read all published hours in one database
query. Each hour keeps its own immutable, hash-bound codebook and snapshot
identity; the API reports these as `lead_sources`. The 250-million-entry
codebook allowance is aggregate across the run, checked before publication and
reader allocation. Grid descriptors share SHA-256-addressed hard links. Readers
reuse decoded books across manifest advances.

A poll allows two regional model extractors and one global model extractor,
with one hour at a time per model and at most 16 map tasks per extractor. Its
work budget limits starting additional hours; already active hours drain.
`watch.lock` prevents duplicate pollers and `update.lock` protects each poll.
Private per-hour journals preserve source checkpoints and table identity across
process restarts. Completed hours are skipped; an unpublished partition can be
replaced on retry without changing any published hour.

## Retention and API contract

The catalog separates `current`, `building`, `complete_runs` and `hrrr_long`.
Complete archives are ranked by initialization and deduplicated before keeping
two per model. HRRR's latest complete 48-hour run is protected independently,
so its complete archive contains two or three distinct runs. Recent partial
runs, plus a pending long HRRR run, are tracked separately. Superseded manifests
have a ten-minute grace period. Garbage collection follows manifests, child
books and shared-table references, dropping only explicitly owned service
tables. Legacy complete tables remain readable without rewriting them.

`/forecast` defaults to the newest validated run, including partial runs.
`forecast_complete`, `available_leads`, `expected_leads`, `missing_leads` and
`freshness` describe its actual progress and horizon. Partial runs use the
existing age limit without requiring a complete forecast's remaining horizon.
Explicit `run=latest_complete`, `previous_complete` and HRRR's `long` select
complete archives and report their freshness even if stale. Default selection
still rejects stale runs. `/runs?model=MODEL` exposes retention roles; `/health`
reports complete-backup counts and HRRR long-run retention.

Full audits of published descriptors preserve the old validation marker until
success. They hold a shared catalog lock and validate through private
hard-linked descriptors, then replace the marker atomically. Each hour retains
its original `source-validation.json` proof even after later independent audits.

`update` performs one incremental poll; `watch` polls continuously and is
started by `service`. `update --batch` or `--pin-root` explicitly preserves the
older complete-run benchmark workflow. `adopt --retain-only` imports a complete
historical backup without changing the selected run. See
[`native/README.md`](../../native/README.md) for commands.

## Live rollout and memory budget

Before changing the service, a private live GFS run published leads 0–3, then
resumed in a second process to add leads 4–5, without acquiring completed hours
again. A four-model API preview passed 36 HTTP/database profile checks,
malformed and outside-domain inputs and gzip handling. Independent audits of
published GFS and HRRR partial manifests also passed.

RRFS initially had one complete run. A full 06 UTC backup was acquired and
loaded before cutover, then independently cloned, validated and retained with
`--retain-only`. The 12 UTC selection stayed unchanged. Its full 85-lead load
was 141.86 seconds internally; adoption and its independent validation took
79.18 seconds. The temporary original table and 22,125,569,023 bytes of weather
staging were removed only after the retained clone served all 85 hours.
The service now has two actual complete archives for every model.

The first live poll saw an incomplete RRFS 18 UTC source with only 28 of 85
hours ready and a nearly complete HRRR 18 UTC source with 48 of 49 ready. Later
polls discovered the new hourly HRRR 19 UTC run while it had only its earliest
hours. All four models published and advanced partial runs while their complete
archives remained selectable.

Concurrent regional loading exposed failures under the original 6 GiB
ClickHouse process budget. The failed hours remained unpublished and resumed
successfully. At the user's request, the server budget was increased to
**12 GiB** on this 32 GB host, keeping parallel loaders and the unchanged 2 GiB
per-query limit. Existing periodic allocator purges and bounded workers remain. Optional stage
prefetch now reserves 18 GiB beyond input bytes (12 GiB server, 4 GiB client,
2 GiB margin), otherwise streaming normally.
The enlarged limit is a ceiling, not a reserved allocation. Subsequent live
polls and all 49 tests passed under this budget.

The evidence directory is
[`docs/benchmarks/2026-10-18-incremental/`](../benchmarks/2026-10-18-incremental/).
It distinguishes initial 6 GiB failures from the final configuration and
includes live progress, archive selections, full-grid publication/retry tests,
independent audits and cleanup records. These observations validate incremental
publication and retention; they do not claim that the newly building runs have
already completed or establish a long-term ingestion SLA.

## Final installation checks

The final executable SHA-256 is
`5b36c3fbbb42a0fe97ffe2b0d124e97b12146b2965dad271c17993a4f5ee373e`.
Formatting, warning-free Clippy and all 49 tests (including the real-database
full-grid publication/retry test and complete-only archive guards) pass.
The installed service passed another 36 HTTP/database profile checks after a
controlled restart. The live configuration reports a 12 GiB server limit.

A three-minute window recorded 181 one-second samples and 37 HTTP health checks
with no errors, plus successful incremental poll reports. Observed server RSS
reached 1,239,867,392 bytes; minimum observed host `MemAvailable` was
21,608,779,776 bytes. These asynchronous samples can miss brief peaks and do not
replace the database's enforced ceiling.

After restart, the first completed poll added seven validated hours with no
failures. The restart audit verifies that all pre-restart published books are
unchanged, each old published hour still has exactly one row per spatial tile,
and all complete backups and the long HRRR archive remain protected. The
post-restart database query log records no exceptions at that audit's cutoff.
The supervisor continues polling; current-run progress in the evidence is a
timestamped snapshot rather than a claim of final completion.
