# RRFS ingestion and uniform codebook allowance

The native Rust service adds RRFS to HRRR, GFS and AIFS. Default six-hour
refreshes include all four models; `update --models rrfs` updates RRFS alone.
The Next.js weather provider remains Open-Meteo.

Every model now has the same **1 GB decoded codebook allowance**:
250,000,000 Float32 entries, or 1,000,000,000 payload bytes. This applies across
all fields in one codebook, during source checkpoint assembly, writing and
reading. Checked arithmetic and the reader's checks before allocation remain.
The 65,536-slot bound and the legacy hash builder's separate 32-million-entry
per-field limit remain. The ClickHouse server/query limits remain 6/2 GiB.
This allowance describes palette data, not total process memory.

## Source and retained selection

The [Dynamical RRFS catalog](https://dynamical.org/catalog/noaa-rrfs-forecast-84-hour-virtual/)
provides the virtual NOAA forecast used by this ingestion. The pinned source is
`noaa-rrfs-forecast-84-hour-virtual`, snapshot `S77EM0FSR50Q18R443Z0`, initialized
2026-10-18 12:00 UTC.

The selection retains 1,638,757 native cells inside 24–50°N, 125–66°W, all
85 hourly leads from 0 through 84 hours, and 30 pressure levels from 1000 down
to 275 hPa in 25 hPa steps. Pressure fields are geopotential height, temperature,
U/V wind, relative humidity and dew point. Surface fields are 2 m temperature
and dew point. This is 139,294,345 logical cell/lead rows and 25,351,570,790
Float32 values. It does not select the source's other variables or higher
stratospheric pressure levels.

RRFS and HRRR have identical source X/Y axes, spherical LCC projection and
retained coordinate bytes in this snapshot. The compressed coordinate-file
SHA-256 is `4c1bc0196a1b89c04bc6c8e896966bc5e41a764183b31f6b6d0903f16bcbd49f`.
Winds remain explicitly grid-relative. The API exposes native fields; this
does not implement cloud estimation, wind rotation or the application's
meteorological adapter.

## Acquisition and storage choices

RRFS's persisted one-million-reference Icechunk metadata cache cannot retain
the required arrays together. The first complete reference check took
344.09 seconds. A bounded eight-million-reference cache at initial repository
open reduced the observed check to 3.36 seconds. This is separate from the
Float32 codebook allowance. Weather payload caching in Icechunk stays disabled;
the on-disk source cache still retains only unfinished leads. Icechunk 2.3
`reopen` reuses its existing asset manager, so setting this cache only at reopen
does not replace the original cache. Other models' source cache settings stay
unchanged.

The first 14 complete leads were checkpointed before the codebook allowance
was increased. Their measured cardinalities projected about 149 million
entries for the full selection, above the former 128-million-entry allowance.
The final ingestion resumes those checkpoints; no completed lead needs to be
downloaded again. A proposed raw fallback was not run.

Spatial palettes preserve exact Float32 bits and are sorted before encoding.
When all slots fit UInt16, the loader uses the existing 256-cell tile format.
RRFS uses 8,192-cell batches, six workers, two readers and four active-insert
permits. The smaller batch keeps its 85-lead Native blocks below HRRR's measured
49-lead per-block size. All frame, full-cell-ID, physical-type, array-shape,
per-slot index, padded-lane and staged sample checks remain enabled.

## Operation

```sh
meteogram-native update --root "$HOME/.local/share/meteogram-native" --models rrfs
curl --noproxy '*' --compressed \
  'http://127.0.0.1:18080/forecast?model=rrfs&lat=41.013&lon=-74.027'
```

RRFS publication requires a complete matching schema, age at most 24 hours and
at least 48 forecast hours remaining. The updater still tries the latest then
the preceding initialization. `/health` requires all four supported models to
be fresh and checks database connectivity. RRFS admission reserves 160 GiB
of working space plus the shared 20 GiB free-space floor.

## Measured complete run

These measurements cover the complete retained RRFS selection above, on this
home Linux host. They are not a partial-tile or projected full-run benchmark.
The source was acquired in two checkpointed segments with deliberate pauses
for the codebook change; their sum is not one uninterrupted end-to-end clock.

| Measurement | Result |
|---|---:|
| Successful source GRIB payloads | 21,660,349,229 bytes in 15,470 chunks |
| Initial 14-lead fetch command | 140.06 s; deliberately stopped before lead 14 |
| Resume command, reusing all 14 leads | 719.29 s |
| Sum of recorded completed-lead work | 841.02 s |
| Compressed UInt16 weather staging | 21,701,930,229 bytes |
| Codebook entries | 150,185,270 |
| Codebook Float32 payload | 600,741,080 bytes, 60.1% of the 1 GB allowance |
| Compressed codebook | 385,950,417 bytes |
| ClickHouse physical tile rows | 6,402, preserving all 1,638,757 cells |
| ClickHouse table after load | 21,807,277,418 bytes |
| Fresh-table load command | **146.71 s** |
| Load's internal total / final checks | 146.53 s / 3.32 s |
| Independent full-table audit with staged samples | **82.08 s** |
| Source resume / loader / independent audit peak client RSS | 3,535,180 / 3,120,992 / 668,872 KiB |

The load clock includes stage reads, decompression, packing, encoding, inserts,
all 201 inserted-range validations, allocator purges, full key checks and final
sample comparisons. It excludes source acquisition and the later independent
audit. Worker time totals overlap and must not be summed as wall time. This
21.7 GB source streams instead of using the loader's ≤8 GiB input-prefetch path.
Source traffic counts successful completed-lead GRIB bytes, excluding metadata,
HTTP overhead and unobserved retries.

The loader proves all codebook values finite and checks every stored index
against its own field/level/lead palette. The independent audit repeats the full
range/shape/type/key scan, including padded lanes: 25,353,968,640 stored indices
represent 25,351,570,790 logical weather values. Ten geographically distributed
profiles are compared bit-for-bit against staged values for every selected lead,
level and field. This is a full finite/index proof plus sampled exact source
comparison; it is not a second bit-for-bit source comparison of all 25 billion
logical values.

The first independent audit exceeded the unchanged 2 GiB per-query cap when
reading eight wide-array streams together. Reducing that to two streams and
one tile per block passed the original table in 24.53 seconds, but a later
publication clone still exceeded the cap on merged parts. Neither failure
published an incomplete forecast. The final implementation audits **one column
at a time**, with two threads and one tile per block, retaining every predicate.
It passed the original table with staged samples in 82.08 seconds, the previously
failing merged clone in 67.72 seconds, and the final publication clone. The final
adopt command, including copying required descriptors/codebook and validation,
took 81.11 seconds; its validation is additional to the 146.71-second load.

ClickHouse's query log records 40 completed final predicate audits across the
original table, merged clone, API preview and publication clone, with zero
exceptions. The largest recorded per-query peak is **692,598,019 bytes** under
the unchanged 2,147,483,648-byte limit. Query-condition caching can skip granules
on repeated predicates over immutable parts; the query-memory evidence records
actual read rows and does not describe every repeated check as a cold scan.

One-second server telemetry began partway through loading and includes the
failed broad audits as well as the initial successful rerun: its maximum observed
resident memory was 2,476,630,016 bytes. It is not a continuously measured
load-only peak. The 6 GiB server cap remains.

The full codebook SHA-256 is
`67654bf2fce748019c4fe9e8a99c1c398233df3a32b9351eb82b8849f1fc722f`.
The final tested executable SHA-256 is
`b607194bd6b17558a8306781776532d11e9d371d082e5c0416492bc37f170624`.

## Publication, service checks and cleanup

The final executable is installed at `$HOME/.local/bin/meteogram-native` and the
local supervisor is running with all four models. Catalog generation 21→22 adds
the owned revision `rrfs_20261018190636_ef01686e`; the three existing current
entries are identical. The existing six-hour schedule is retained, with the next
update due 2026-10-18 19:19:29 UTC. Its default update command includes RRFS;
this task did not wait for that future scheduled acquisition.

Before publication, a private four-model API preview passed full database
validation, 36 point-profile comparisons with direct database reads, four
malformed-input checks, four outside-region checks and gzip handling. The final
installed service passed those HTTP/profile checks again. All 36 served weather
row hashes match the preview, including all 27 existing HRRR/GFS/AIFS profiles.
A final post-cleanup RRFS request returns all 85 leads and 30 levels with the
explicit grid-relative wind frame; all four models report healthy.

During source acquisition/loading and the first preview, the old service served
1,036 monitored requests per existing model with zero errors (3,108 total).
These are loopback requests at one fixed New York point, not WAN or comprehensive
load measurements. The planned publication/binary restart caused a measured
**6.02-second health gap** at approximately 100 ms polling. That interval excludes
cold first-forecast latency. Final review also fixed the store-comparison
diagnostic's hardcoded three-model assertion: it now requires matching, nonempty
model sets. A four-model same-catalog smoke check passes with one profile per
model; this checks diagnostic compatibility, not independent source equality.
Installing that final diagnostic build required another monitored restart with
a **1.55-second health gap**. The final installed API then passed all 36 profile
checks again. No zero-downtime claim is made for these upgrades.

After installed-service comparisons passed, cleanup dropped only this task's
initial `rrfs_initial_20261018_12` table and failed private clone
`rrfs_20261018190152_4e17ff13`, and removed its failed revision, preview and
425 staging files/directories. This removes 22,139,672,559 logical staging bytes;
physical free-space movement also includes other local database activity.
All **12 pre-existing benchmark/service tables** retain their original row
counts, alongside the published RRFS table. The active complete codebook and
coordinate descriptors remain available; the retained diagnostic codebook is a
SHA-verified hardlink to the published copy. Staged weather payloads are no
longer retained for another source comparison.

`cargo fmt --check`, warning-free `cargo clippy --all-targets`, all **44 tests**
(including isolated ClickHouse integration tests), and the optimized release
build pass. Tests cover RRFS schema/freshness, source manifest-cache isolation,
uniform oversized-book rejection before allocation, RRFS automatic tiled load
and independent validation, alongside the existing corruption and lifecycle
gates. This adds native ingestion and serving; no Next.js provider switch,
RRFS menu entry, cloud estimate or wind rotation is deployed.

The [machine-readable evidence](../benchmarks/2026-10-18-rrfs/README.md) contains
source/preflight coverage, resource clocks, dictionary cardinalities, full
validation proofs, profile hashes, table preservation, query memory, service
restart and cleanup results. Large weather payloads and database files stay
outside Git.
