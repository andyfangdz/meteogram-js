# Native ingestion: packing locality, source preflight and bounded cache

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

The Rust service now packs small groups of cells across all forecast leads,
checks a pinned snapshot's required map references before acquisition, and
removes encoded maps after durable staging checkpoints. The physical forecast
schema, precision, regional coverage, API contract and six-hour schedule are
unchanged.

This follows the [Rust service migration](2026-10-18-native-rust-service.md).
Measurements use the same eight-core ARM host, ClickHouse 26.9.20.23 and Rust
1.99.1. Evidence is in
[`../benchmarks/2026-10-18-native-ingestion-optimization/`](../benchmarks/2026-10-18-native-ingestion-optimization/).

## Packing

The previous NEON kernel traversed every cell in a batch for each group of four
leads. That repeatedly revisited a large output buffer. The new traversal
finishes sixteen cells across all leads before moving to the next group. The
deinterleave, transpose, finite-bit checks and output indexing are unchanged.
The portable non-ARM implementation remains available.

The resident synthetic benchmark uses an explicit warmup, seven timed repetitions
per kernel, rotating execution order and a bit comparison after every call.
Allocation and comparisons are outside the timed kernel. Both implementations
read identical inputs and write the same output shape.

| Pressure-field batch | Original median | Sixteen-cell tiles | Ratio |
|---|---:|---:|---:|
| HRRR: 4,096 cells × 30 levels × 6 fields × 49 leads | 84.52 ms | 29.25 ms | 2.89× |
| GFS: 2,048 cells × 30 levels × 6 fields × 141 leads | 150.08 ms | 34.16 ms | 4.39× |

These are resident packing measurements, not whole-refresh speedups. The full
loader also waits for staged-file I/O, encodes Native blocks, inserts/compresses
data and validates the complete table. Packing work timers are elapsed wall
times and include memory-mapped-file waits.

The experimental Linux loader requested `MADV_WILLNEED` for the following input
batch while packing/inserting the current batch. The first batch is also
hinted at startup. Read-ahead stayed bounded to the next batch and created no
additional packed-output queue. Advice failures were counted and treated as
optional hints; data checks remained mandatory. It did not establish a complete
load-time improvement and was removed. The experiment's patch and binary hash
are retained as evidence; the production loader does not issue these hints.

## Complete-file load measurements

All three full datasets were freshly acquired for initialization
`2026-10-18T00:00:00`, then the HRRR loaders read the same staged files, with the
same 4,096-cell batches, T64 schema and complete validation gates. All source
downloads finished before the baseline started. The HRRR input contains
1,638,757 cells × 49 leads, or 80,299,093 logical cell/lead records.

| HRRR implementation | Insert/packing span | Validation | Complete load |
|---|---:|---:|---:|
| Baseline, commit `7d88dd4` | 319.69 s | 31.91 s | 351.70 s |
| Sixteen-cell packing | 312.57 s | 41.57 s | 354.27 s |
| Packing plus experimental read-ahead | 314.18 s | 51.72 s | 365.95 s |

The traversal-only total is confounded by a follow-up build overlapping its
validation. The baseline and read-ahead loads had no concurrent compilation.
The runs were sequential; filesystem cache, thermal state and background disk
behavior were not reset, and the full pair was not repeated. These observations
do **not** establish a healthy full-HRRR-load speedup. The resident kernel gain
does not remove the staged-file I/O and database work.

The measured resident packing improvement is retained. Read-ahead hints are
excluded from the shipped implementation. The clear operating gains here are
early rejection of incomplete runs and lower temporary cache retention.

The load counters overlap: packing includes file waits, while Native encoding
and ClickHouse insertion run against the producer concurrently. Do not add the
individual work counters or interpret them as CPU time. No new complete-refresh
speedup, physical cold-cache result, disk endurance improvement or service SLA
is claimed.

## Reject incomplete runs before map acquisition

The source reader constructs all required map keys and checks their references
in the pinned Icechunk manifests, with at most sixteen checks in flight. It
writes `preflight.json` before acquiring any forecast maps. A missing reference
fails the candidate and preserves the existing updater's fallback behavior.
This does not verify that referenced remote objects can be fetched or decoded;
all existing data and publication gates remain required.

A real incomplete HRRR initialization was replayed from the same immutable
snapshot that failed during the earlier scheduled refresh:

- Initialization: `2026-10-18T06:00:00`.
- Snapshot: `TFKYRS930QCYAQG503D0`.
- Required references: 8,918; missing references: 3,272.
- Earlier attempt: 234.68 seconds from selection to failure. Its 25 completed
  leads had already downloaded 3,291,067,849 encoded bytes, plus uncounted
  partial-lead traffic.
- New attempt: 4.17 seconds for the complete process, including 1.46 seconds
  checking references. It rejected the run without forecast-map downloads or
  weather staging files. Coordinate and repository metadata reads still occur.

The early check adds a small cost for complete snapshots. Actual complete GFS
and AIFS acquisition also passed the new check in this experiment.

## Retain only unfinished map caches

For each lead, the reader syncs both staged arrays, atomically writes and syncs
the checkpoint, then deletes that lead's encoded map cache files. An interrupted
lead retains its cached maps. Resume also removes completed-lead maps left
behind between checkpoint and cleanup. Staging remains available until loading,
full validation and publication succeed.

Resuming the complete HRRR and GFS extractions removed 24,075,332,535 encoded
bytes across 34,580 files. All staged forecasts remained loadable. A fresh,
complete AIFS extraction exercised eviction after every lead: it downloaded
851,594,073 encoded bytes across 31 leads, while quarter-second sampling observed
a peak cache of 28,816,077 bytes and 42 files. The largest complete lead contained
30,485,215 encoded bytes, so sampling did not capture the exact maximum. No
encoded map files remained after completion.

This reduces temporary retention. Maps are still downloaded and initially
written once; network volume and cache write bytes are not reduced by eviction.
Raw staging, database retention and conservative admission/reserve budgets are
unchanged. A complete encoded-map archive is no longer retained for offline
replay after staging is checkpointed.

## Verification and server installation

All 221 forecast leads and 35,882 source maps were acquired for the same
initialization. The retained source payload totaled 24,926,926,608 encoded bytes;
raw staged values totaled 61,141,736,264 bytes, excluding file headers.

| Model | Pinned snapshot | Native cells | Leads | Pressure levels | Logical cell/lead records |
|---|---|---:|---:|---:|---:|
| HRRR | `KB3VN67D9882NQ6F549G` | 1,638,757 | 49 | 30 | 80,299,093 |
| GFS | `CGTMC4JWR6DN9JSYTCR0` | 24,885 | 141 | 30 | 3,508,785 |
| AIFS | `TMCJG4BMMWRVTBVBJK7G` | 24,885 | 31 | 8 | 771,435 |

The shipped packing implementation completed full finite/type/length/key
validation on all three complete datasets, plus exact staged samples. Every
cell ID and coordinate matched the published store; 100 complete profiles per
model matched bit-for-bit, covering every lead and pressure level in each
sample. An independent HRRR baseline table also matched the sampled profiles.
The rejected read-ahead variant passed the same data checks. Both variants
passed 27 complete HTTP comparisons with the published API, invalid/outside
coordinate checks and gzip verification.

All 25 Rust tests passed, including three actual ClickHouse integration tests.
Coverage now crosses cell-tile and 4,096-cell loader boundaries, includes a short
final batch, preserves signed zero/subnormal/maximum finite bits, rejects
non-finite values, checks reference concurrency/error propagation, and preserves
uncheckpointed cache files. Formatting and Clippy with warnings denied passed.

The final release build is byte-identical to the traversal-only binary used for
the complete three-model data verification. Installed SHA-256:
`b76b2dac24a2efe66032bab61d3c008ce2cd12fe86384e214939c65670f64e6c`.
It replaces `~/.local/bin/meteogram-native` atomically. The running supervisor
already stores that executable path for future workers, so the next scheduled
update uses the optimized source reader and packer. No database/API restart was
needed. Current forecasts, catalog generation 18 and the saved refresh schedule
were preserved; all 27 published profile hashes still match after installation.

During acquisition, the experiments and cleanup, 2,625 fixed-point requests to
the published API succeeded with zero errors or changed revision hashes. The
observed p95 values were 41.0 ms for HRRR, 62.8 ms for GFS and 27.1 ms for AIFS.
The GFS maximum was 6.40 seconds: tail latency remains unresolved. This was
loopback traffic with a fixed catalog, not a live-publication or WAN test.
Minimum observed free space was 195.86 GB and host available memory was 24.70 GB;
the experiment temporarily retained multiple duplicate tables.

Only the seven explicitly named experimental tables and 442 newly acquired
staging arrays were removed. Protection checks preserved all twelve published,
retired and historical benchmark tables and their row counts. Experimental
catalogs were cleared after their descriptors and verification evidence were
saved. Selected metadata and small reports are committed; full arrays, database
parts, binaries and encoded maps remain outside Git.

The application's meteorological provider remains Open-Meteo; these changes
apply to the native forecast service. The existing direct Rust supervisor
continues running; this work does not add new reboot/service-manager evidence.

## Reproduce the isolated packing test

```sh
cargo build --release --locked --manifest-path native/Cargo.toml
meteogram-native benchmark-pack --cells 4096 --leads 49 --repeats 7 \
  --output hrrr-packing.json
meteogram-native benchmark-pack --cells 2048 --leads 141 --repeats 7 \
  --output gfs-packing.json
```

For complete-file comparisons, pin the recorded model metadata, acquire into a
new isolated data root, then run the baseline and new `load` commands against
those same staged files with unique table names. Stop competing acquisitions
and compilation before timing. Use `compare-stores` and `check-api` before
removing experimental tables. Existing serving tables must never be rebuilt in
place.
