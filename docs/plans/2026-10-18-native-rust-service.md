# Native forecast service: full Rust migration

The native forecast service now has one Rust/Cargo codebase and one installed
application executable, `meteogram-native`. Source acquisition, staging,
ClickHouse ingestion, validation, catalog publication, retention, HTTP serving,
periodic supervision and verification no longer require Python. The separate
Next.js application's provider remains Open-Meteo.

## Implementation

The application is in [`native/`](../../native/README.md). Cargo locks the source
libraries to Icechunk 2.3.0 and Gribberish 1.8.4. The tested toolchain is Rust
1.99.1; the project pins it for Cargo/CI. Codecs compile through Cargo, and the
installed ARM executable links only the normal Linux C/math/compiler runtime
libraries. It does not load a Python interpreter or compile a packing library
at startup. ClickHouse remains its existing separately installed server binary.

The source reader applies the Zarr codec pipeline directly, including Blosc
coordinate decoding, longitude wrapping, north-up GRIB ordering and Float64
scale/offset conversion before the existing Float32 cast. Stored Float32 bits
are preserved exactly. A source change in dimensions, chunk shape, fields,
pressure levels, regional cell counts or forecast cadence fails validation.
Source maps are fetched only by background acquisition.

The loader preserves the `cell_profile_v1` format from the preceding optimization:
one physical row per native cell, pressure arrays ordered level/lead, surface
arrays ordered lead, and `Array(UInt32) CODEC(T64,ZSTD(1))` for Float32 bits.
The NEON/portable Rust packing routine and Native-format block writer are now
called directly from Rust. A bounded producer overlaps packing with insertion.
Legacy cell/lead tables remain readable, so current forecasts and rollback
history do not need a data migration.

The service retains the same catalog and operating policies: immutable unique
candidate tables, complete validation before atomic/fsynced publication, a
shared reader lock through the DB query, exclusive publication/GC locks, one
updater lock, current/previous revisions, ten-minute retirement grace, and
explicit ownership before deletion. Failed unpublished work is recovered;
unreferenced failed evidence expires after seven days. Prefetch tasks are
cancelled and drained before an error path releases the updater lock. Staging
files are synced before their durable checkpoint.

The supervisor starts its own ClickHouse/API/update children, keeps their process
groups separate, restarts failures and persists the six-hour success / 30-minute
failure schedule. API requests preserve native fields, units, selected cells,
source identity, gzip, freshness checks and 400/404/503 behavior. The API remains
loopback-only. Native weather normalization and app integration remain separate.

## Compatibility and source verification

Before cutover, a Rust API and the existing Python API returned matching complete
payloads at 27 city, interior and edge points across all models. Weather values
matched exactly; revision/query duration were excluded and coordinate round-off
was bounded to 10⁻⁹ degrees. Invalid inputs, points just outside the regional
bounds, native grid edges and gzip were checked.

A failed initial acquisition exposed an Icechunk 2.3 object-store S3 option
interaction: persisted `anonymous=false` overrode explicit anonymous credentials,
causing an EC2 metadata lookup. The port now explicitly keeps STAC-authorized
public S3 containers anonymous. A regression test covers the configuration;
real HRRR, GFS and AIFS GRIB probes subsequently downloaded and decoded
successfully. No host AWS credentials are needed.

The full verification cycle uses fresh local output/cache directories and pins
the same 2026-10-17 18Z snapshots as the retained reference forecasts. Upstream
transport caches are not controlled. This is a functional full-volume check;
small compilation/integration checks also ran on the host during acquisition,
so its elapsed time is not a controlled language performance comparison.

The final executable additionally passed a fresh AIFS lead-0 source acquisition,
durable staging, Native insertion and full validation, including ten complete
bit-exact staged/database samples. This exercises the final checkpoint and
operational guards added while the full cycle was running.

## Completed full-cycle check

The fresh pinned cycle completed in **826.924 seconds (13m47s)** with no model
failures. It acquired all 221 forecast times and 35,882 GRIB chunks, totaling
24,963,345,620 encoded payload bytes. The three tables together retained:

| Model | Logical cell/lead rows | Physical cell rows | Resident bytes |
|---|---:|---:|---:|
| HRRR | 80,299,093 | 1,638,757 | 24,322,228,889 |
| GFS | 3,508,785 | 24,885 | 1,540,907,222 |
| AIFS | 771,435 | 24,885 | 72,212,996 |
| Total | 84,579,313 | 1,688,527 | 25,935,349,107 |

All full-table ID/type/length/finiteness gates passed. Ten complete profiles per
model matched staged Float32 bits. An independent comparison then checked every
retained cell ID and coordinate, plus 100 complete profiles per model against
the existing serving store. All weather values matched exactly, including signed
zero; the maximum HRRR coordinate difference was 5.69×10⁻¹⁴ degrees. GFS/AIFS
coordinates were identical.

The candidate Rust HTTP server matched 27 complete profiles against the existing
API. Each model then rolled back from the new profile layout at 18Z to a borrowed
legacy revision at 12Z and restored 18Z. All 162 transition-check HTTP profiles
passed, and every restored weather hash matched the initial candidate. Borrowed
references remained explicitly unowned; their source tables were preserved.

During the isolated build, the existing Python API served 1,023 monitored
requests with zero errors. The fixed-point p95 timings were HRRR 106.2 ms, GFS
77.9 ms and AIFS 29.7 ms. The maximum GFS response was **3.994 seconds**. This
observes contention while the serving catalog remained unchanged; it neither
proves a publication-latency bound nor shows that the earlier tail-latency issue
is resolved. The observer sampled at least 244.44 GB free disk and 26.05 GB host
available memory. These are host-wide observations, not per-process RSS or device
endurance measurements.

## Running host state and cleanup

The installed executable is `~/.local/bin/meteogram-native`; the running
supervisor, API and future refresh jobs use Rust. State remains in
`~/.local/share/meteogram-native`. The existing six serving tables, catalog
generation 15 and next refresh at **2026-10-18 07:29:57 UTC** were preserved.
Both retained forecasts still total 169,158,626 legacy physical/logical rows.
New refreshes use the profile format automatically.

After cutover, the full retained-current-table checks passed within the 2 GiB
query limit; legacy uniqueness is verified in bounded 32,768-cell ranges. The
live API again passed all 27 profile checks. A controlled database shutdown
exercised the Rust supervisor: both database and API PIDs changed and health
returned after an observed 33.3 seconds. Catalog, schedule and weather hashes
remained unchanged. This was a planned local restart, not a zero-downtime test.

The updated user unit is installed and statically verified, with its existing
`default.target.wants` dependency retained. The user service-manager bus is not
available inside this execution environment, so the active instance runs under
the direct Rust supervisor. Reboot/login activation was not exercised.

Cleanup dropped only five explicitly identified experimental tables, after
checking that their names were disjoint from every serving current/previous/
retired reference. All six serving tables and original benchmark tables remain.
The experimental catalogs were cleared after saving their descriptors; source
reports and small metadata remain for inspection. The large full-cycle source
cache/staging had already been removed after successful publication. Temporary
API children created by `check-api` were stopped by their owning Rust command.

## Tests and maintenance

All 22 Rust tests passed, including three real ClickHouse integration tests.
Coverage includes SIMD/vector tails, signed zero/subnormal/maximum finite bits,
non-finite rejection, Native database round trips, corruption gates, clone
independence, ownership/retention, source credential configuration, Celsius
conversion, schema/identity/freshness gates, old metadata compatibility and
cancellation before unlocking. Formatting and Clippy checks passed. A Cargo CI
workflow runs unit tests, formatting and lint; database tests remain explicitly
opt-in when an isolated database is available.

The retired Python scripts, requirements and runtime shared-library adapter are
removed from the current branch. Their historical versions and original
benchmark commands remain in commit `5f4ae08` and earlier history. Current
operations, inspection, source probes, HTTP/data verification, query benchmarks
and staging compression use the Rust CLI documented in `native/README.md`.

Evidence is stored under
[`docs/benchmarks/2026-10-18-native-rust-service/`](../benchmarks/2026-10-18-native-rust-service/).
Large datasets, caches, database files and binaries remain outside Git.
