# Rust, SIMD and profile storage — measured home-server optimization

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

The full pinned HRRR/GFS/AIFS build completed in **16m00s**, versus **20m11s** in
[the previous optimization](2026-10-17-native-ingestion-optimization.md): **20.7%
less elapsed time**. Active data occupied **25.935 GB**, versus **35.301 GB**,
a **26.5% reduction**. The source data, geographic coverage, forecast horizons and
Float32 values are preserved.

Rust now handles the packing/finite-check kernel and ClickHouse Native-format
encoding. Source acquisition and service orchestration remain Python; this is an
optimization of the ingestion core, not a wholesale replacement of the service.
The measured improvement combines the new physical model, binary writer,
packing/insertion overlap and earlier source acquisition. It is not an isolated
benchmark of Rust versus Python as languages.

## Implementation

`profile_store.py` writes one physical row per native grid cell, containing its
complete forecast. Pressure fields use `[level, lead]` order, with forecast time
contiguous; surface fields use lead order. Float32 IEEE bit patterns are stored
as `Array(UInt32) CODEC(T64,ZSTD(1))`. This is a bit reinterpretation, with no
quantization or reduced precision. The metadata-aware reader reconstructs the
existing lead/pressure HTTP representation. Raw SQL consumers must interpret the
bits rather than use the stored integers as weather values.

| Model | Previous logical/physical cell-lead rows | New physical cell rows | New active storage |
| --- | ---: | ---: | ---: |
| HRRR | 80,299,093 | 1,638,757 | 24.322 GB |
| GFS | 3,508,785 | 24,885 | 1.541 GB |
| AIFS | 771,435 | 24,885 | 0.072 GB |
| Total | **84,579,313** | **1,688,527** | **25.935 GB** |

All **84,579,313 logical cell/lead records** remain available. The physical model
matches a point-profile request; it is less convenient for ordinary numeric SQL
analytics on individual times. Tables retain geographic partitions of 32,768
native cell IDs and use a four-cell index granularity.

`profile_pack.rs` implements a fused ARM NEON kernel. It deinterleaves six fields
and transposes four levels/four forecast times together, checking finiteness via
IEEE exponent bits in the same pass. Other field counts and tails are handled
explicitly. Rust also builds a plain HTTP Native block with array offsets and
contiguous values. Python borrows its buffer through synchronous insertion,
then frees it, avoiding Arrow parsing and an extra Python wire-buffer copy.

One worker prepares the next batch while the current batch is encoded/inserted.
At most two packed batches and one encoded request are retained. Batches are
4,096 cells for HRRR/AIFS and 2,048 for GFS. The updater also starts GFS acquisition
alongside HRRR acquisition, with both full disk budgets admitted first. It can
run two initial extractors, then one extractor alongside a loader. Cancellation
joins workers before releasing updater locks; the 20 GiB reserve and conservative
admission budgets remain unchanged.

The Rust implementation uses only std, built with rustc 1.99.1 on this ARM host.
It has no runtime crate downloads. Its shared library is cached outside Git by
source/compiler/architecture. `METEOGRAM_RUSTC` selects a compiler; without one,
NumPy packing and Arrow insertion remain the portable fallback. The emitted
binary was inspected and contains NEON `ld3`, `trn1`, `trn2`, `uzp` instructions.
No fast-math is enabled.

## Full-cycle measurements

The experiment reacquired the same **2026-10-17 18Z** source snapshots as the
previous run. It started at **03:47:45 UTC** and finished at **04:03:45 UTC** on
October 18, crossing local midnight. Metadata selection was pinned in both
experiments; each built a complete separate catalog while the serving catalog
remained intact. These are measured runs on the same host, not a guaranteed
runtime for different source/network conditions.

| Model | Previous acquisition | New acquisition | Previous load/validate/publish | New load/validate/publish |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 8:28 | 8:47 | 11:03 | **6:33** |
| GFS | 10:04 | 10:46 | 0:28 | **0:13** |
| AIFS | 0:36 | 0:37 | 0:02 | 0:02 |

HRRR loading became **40.8% faster**. Its source acquisition was slightly slower,
as was GFS acquisition, but GFS now finishes well before HRRR publication.
Earlier acquisition prevents GFS from becoming the new critical path after the
HRRR loader improves. AIFS acquisition overlaps GFS loading.

HRRR work measured **261.05 s packing**, **12.11 s encoding**, **212.49 s inserting**
and **32.66 s validating**. Packing overlaps insertion: only **133.54 s** were
uncovered packing waits, and the complete loader took **391.69 s**. Do not sum
these overlapping work times and call that sum wall time. Raw staging reads
remain substantial; SIMD has not removed their I/O cost.

Screening tested ordinary Float32 arrays, alternative axes, Gorilla/LZ4 and
UInt32-bit Delta/T64 codecs. On 16,384 HRRR cells with all 49 times, T64 reduced
334.67 MB to 237.42 MB. Gorilla grew to 445.02 MB and was rejected; LZ4 used
447.56 MB. The final Rust Native writer's write-only sample took 2.60 s versus
3.35 s for the T64 Arrow prototype. Sample/core timing boundaries differ from
full validation; the **16m00s full build** is the end-to-end comparison.

NumPy already uses NEON here. Rust's packing microbenchmarks do not demonstrate
an order-of-magnitude gain over NumPy; the larger benefits come from modeling,
writing, and overlap. Auxiliary small integration/sample checks ran during early
source acquisition. Resource/server-event samples therefore include those
checks and should not be treated as an isolated hardware saturation experiment.

## Correctness, query behavior and deployment

All source maps were read again: **24,963,345,620 encoded payload bytes**, with
**zero map retries**. Full validation compares every sorted cell ID, physical
column types, array lengths, revision identity and finite bits across the entire
table. Ten profiles per model match staging at every field/lead/level, and an
additional **100 complete profiles per model** matched the original serving
Float32 tables exactly. The storage layout is versioned in metadata and bound
to the validation marker before publication.

The serving API handled **1,350 requests with zero errors** during the build.
This observer used one fixed coordinate per model and an unchanged serving
catalog. It does not measure publication-lock latency or establish that the
previous live-update 9.94-second tail has been eliminated. Its maximum response
was 637 ms; HRRR/GFS/AIFS p95 were 106.83/90.85/36.20 ms.

A temporary API returned **42 exact HTTP profiles**, including cities, random
cells and edges. Each model was rolled back from the new profile layout to the
legacy 12Z layout and restored, verifying data and identity. These checks passed
again after a controlled database/API restart, proving the new tables remained
readable after reopening. Automatic restart recovery took **34.13 s**, preserving
all serving current/previous rows, catalog and schedule. The running API reader
now supports both formats; subsequent scheduled updates use the Rust/profile
loader. Existing serving revisions keep their old format until replaced by newer
forecasts. The separate benchmark did not replace production forecast identities.

The final uninterrupted query comparison ran after recovery, using the same
300 random cells per layout, an explicit warm-up pass, 300 timed single-client
queries and 300 four-client queries:

| Model | Serving legacy warm p95 | New profile warm p95 | Serving legacy four-client p95 | New profile four-client p95 |
| --- | ---: | ---: | ---: | ---: |
| HRRR | 10.94 ms | 7.97 ms | 12.45 ms | 11.95 ms |
| GFS | 13.48 ms | 15.15 ms | 15.12 ms | 22.15 ms |
| AIFS | 9.98 ms | 6.70 ms | 11.90 ms | 11.13 ms |

GFS is the query tradeoff: its longer profiles cost roughly 1.7 ms more in the
single-client p95 and 7 ms more with four clients. This is not a universal query
speedup. Initial, not-fully-warm HRRR candidate reads had p95 24.16 ms and one
366 ms outlier; they are separate from the warmed comparison.

**23 tests passed**: ten real/pure pipeline tests, seven scheduling/failure tests,
three packing tests and three coordinate-boundary tests. Coverage includes both
layouts, old/new rollback compatibility, clone independence, GC, partial-source
fallback, cancellation, encoded infinity corruption, signed zero, subnormal and
maximum finite bit preservation, and SIMD tails. Python compilation, Ruff and
rustfmt checks also passed.

Observed minimum free disk was **251.989 GB**, minimum available RAM **25.327 GB**,
and peak inactive native parts **12.518 GB**. Sampling began 33 seconds into
acquisition and covered all loading. Server `OSWriteBytes` increased by 44.104 GB,
including auxiliary checks; this is not NAND wear. Conservative capacity budgets
have not been lowered based on a single cycle.

After all comparisons, only the three owned experimental tables and 98 sample
arrays were removed. Borrowed legacy tables and all serving revisions were
preserved. Free disk returned to **369 GB**, including the newly installed local
Rust toolchain. Small evidence and metadata remain. The app still uses Open-Meteo,
and the native service remains local. Live systemd activation/reboot verification
is unchanged from the earlier deployment.

[Evidence bundle](../benchmarks/2026-10-18-native-rust-optimization/README.md)
contains phase timestamps, validation markers, screening results, emitted SIMD
proof, query/HTTP/rollback comparisons, restart/cleanup verification and tests.
See the [prototype README](https://github.com/andyfangdz/meteogram-js/blob/5f4ae08/scripts/clickhouse-prototype/README.md) for
operation and compiler/fallback behavior. Format references:
[ClickHouse Native](https://clickhouse.com/docs/reference/formats/Native),
[compression guidance](https://github.com/ClickHouse/clickhouse-docs/blob/main/docs/data-compression/compression-in-clickhouse.md),
and [Rust ARM NEON](https://doc.rust-lang.org/core/arch/aarch64/fn.vtrn1q_f32.html).

The subsequent [full Rust service migration](2026-10-18-native-rust-service.md)
replaces Python acquisition, orchestration, serving and operational tooling.
Current build/operation commands are in the [native service guide](../../native/README.md).
