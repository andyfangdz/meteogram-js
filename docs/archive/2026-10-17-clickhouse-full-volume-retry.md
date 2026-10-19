# Full-volume retry: full CONUS geography, incomplete forecast horizon

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

**Later home benchmark completed:** [full-resident HRRR/GFS/AIFS results](2026-10-17-clickhouse-home-resident.md)
now replace the full-run projections and outstanding acquisition limits in this
historical report. The original sample measurements below are preserved.

**The complete forecast-volume retry remains blocked by the workspace's outbound
tunnel.** Access briefly recovered on October 17, then HTTP 503 failures returned
for Dynamical, both other model catalogs, and an unrelated control site. Five
checkpoint-resume attempts also failed. This is not evidence of a ClickHouse
capacity limit or a Dynamical service outage.

We did improve the experiment: **all 1,638,757 HRRR grid cells in the CONUS
rectangle were loaded into ClickHouse for two actual forecast hours**. The
remaining 47 hours and GFS/AIFS volumes were not acquired. No forecasts were
duplicated or synthesized to simulate a full run.

## Actual measurements

Same four-core CPU quota, 16 GiB memory limit, ClickHouse 26.9.20.23 and VM overlay
storage as the [initial benchmark](2026-10-17-clickhouse-prototype-results.md).
HRRR initialization is October 16 at 18Z, snapshot
`8QX1NG8ZDNGCXNFVESV0`. Coverage is the native-grid intersection with 24–50°N,
125–66°W, including surrounding water/land. This is not a political-border mask.

| Measurement                                          |                                 Result |
| ---------------------------------------------------- | -------------------------------------: |
| Distinct locations                                   |                              1,638,757 |
| Actual forecast lead hours                           |                                   0, 1 |
| Pressure levels / vertical fields / surface fields   |                             30 / 6 / 2 |
| Cell/time rows                                       |                              3,277,514 |
| Float32 scientific values                            |                            596,507,548 |
| Raw scientific values                                |                              2.222 GiB |
| Final ClickHouse table, 256-row granules and ZSTD(1) |                              1.207 GiB |
| Load time, including transpose and Arrow transfer    |                          31.13 seconds |
| Load rate                                            |                    105,293 rows/second |
| Server CPU during loading                            |                      53.94 CPU-seconds |
| Client CPU during loading                            |                       8.68 CPU-seconds |
| Sampled server peak RSS during loading               |                               1.01 GiB |
| Server process writes during loading                 |                               4.175 GB |
| Warm query p95, 120 distinct cells                   |                                6.85 ms |
| Direct-I/O query p95, 30 distinct cells              |                               17.61 ms |
| Four clients, 120-query burst                        |      449 requests/second; p95 10.75 ms |
| Reads during loading                                 | 485 queries; p95 23.53 ms; zero errors |
| Local API, six arbitrary coordinates                 |          43–62 ms; roughly 3.7 kB gzip |

Every query returns **two times**, not 49. Direct I/O bypasses the guest page
cache, but does not flush host/device caches. All timings exclude home Internet,
TLS and a public gateway. The reader during insertion queried already-loaded
cells while the remaining cells arrived. It does not test atomic publication of
a replacement forecast. Background source-metadata retries were also running;
they transferred no successful forecast payload during this load.

The table had 18 active parts immediately after insertion and seven at the final
measurement after ordinary merges. No forced `OPTIMIZE` was used. Process write
bytes include merges and logs; they are not a physical SSD-write measurement.

The API successfully queried Atlanta, Seattle, Miami, Los Angeles, Denver and
Boston. Out-of-region and invalid coordinates returned 404 and 400 respectively.
The API remains a native-field prototype: HRRR winds are grid-relative and the
application still uses Open-Meteo.

## Acquisition and verification

The retry downloaded 364 complete GRIB messages for these two times, totaling
**265,368,795 compressed bytes**. The first spatial batch retained 204,844 cells.
Because each GRIB message contains a complete map, its saved payload also covers
every other CONUS cell. Replaying those immutable chunks offline produced the
entire regional dataset with **zero additional source-download bytes**.

Offline decode, gather and raw-array staging took 17.35 seconds in total, with
25.08 CPU-seconds and a process peak RSS of 3.54 GiB. These are local replay
times, not full-region Internet acquisition timings. The failed partial third
time and separate access probes downloaded additional data that is not included
in the two-time successful-payload total.

Verification included:

- Exact Float32 agreement between online extraction and offline replay for all
  204,844 shared cells, both times, every pressure level and field.
- Exact ClickHouse round trips at eight deterministic random cells, checking
  all retained fields/times/levels, plus total and unique-key counts.
- Geographic bounds, ordered unique IDs, and eight disjoint spatial batches
  whose concatenation contains every regional cell exactly once.
- A missing cached source chunk raises an error instead of producing fill data.
- All required values in this two-time regional selection were finite. Maximum
  height conversion error was 0.00049 m; U/V error was at most 0.00000382 m/s.

These checks establish storage fidelity, not complete meteorological validation.

## Revised estimates for the proposed home machine

The geographic sample is now complete, but temporal compression and sustained
full-horizon ingestion remain unmeasured. Scaling two times to 49 gives:

| HRRR full-horizon quantity  | Revised projection | Previous tile-based projection |
| --------------------------- | -----------------: | -----------------------------: |
| ClickHouse storage          |           29.6 GiB |                       32.1 GiB |
| ClickHouse load time        |       12.7 minutes |                    8.5 minutes |
| Lossless compressed staging |           29.0 GiB |                   about 24 GiB |

The two-time full-region staging test produced 1.185 GiB of per-field,
4096-cell-block ZSTD(1) data from 2.222 GiB raw. This is a **0.533 size ratio**,
versus 0.443 in the original four tiles. All compressed blocks round-tripped
exactly. The test measures the layout; the extractor still writes raw `.npy`
checkpoints.

Using the new HRRR ratios for GFS/AIFS solely as an unmeasured scenario gives
about 30.9 GiB for one combined stored cycle and 30.4 GiB for compressed staging.
Two retained cycles, one new cycle, one cycle of merge allowance and compressed
staging total **154 GiB / 165 GB before OS, logs, source caches and free-space
reserve**. The earlier 170–220 GB working-space planning range remains plausible
on the user's 300 GB SSD, but actual peak disk use has not been validated. Keep
the 25–45 GiB HRRR-per-run planning range and gate ingestion on free space.

The larger test also makes SSD-write estimates less comfortable: naïvely scaling
the observed ClickHouse process writes gives **12.3 TB/month for HRRR alone** at
four runs/day, before staging and the other models. This is not an endurance
forecast: this two-time run used 201 relatively small inserts, and a full-horizon
batching policy changes merge amplification. The previous 7–15 TiB/month total
is **not a validated upper bound**. Larger sorted inserts, source-cache eviction
and bounded staging remain important to measure before unattended operation.

The 500/80 Mbps uncapped connection and 32 GB RAM assumptions still look
reasonable. These results do not establish the all-model refresh duration,
49-time response size, or public-serving capacity. Existing electricity scenarios
remain unchanged; this retry establishes no new recurring service charge.

## Disk-bounded full-volume retry tooling

This workspace started with about 28 GiB free, less than the projected complete
HRRR table even before staging. The new `full-volume.py` runner therefore plans
eight spatial batches, each with all 49 real times. It caches each encoded source
chunk by dataset/snapshot, loads and verifies each batch, saves measurements,
then drops that batch before continuing. GFS uses all 141 native times through
180 hours; AIFS uses 31 six-hourly times through 180 hours.

**The eight-batch full-horizon experiment has not completed.** Even when it does,
it will measure aggregate actual volume and batch-sized query performance, not
a simultaneously resident complete HRRR database. A full-resident benchmark needs
more workspace disk. This runner is benchmark tooling, not the home production
ingestion/publication service.

The first two times, partial chunk cache, logs and raw staging remain in
`/workspace/dynamical-benchmark/full`. The local API currently serves the complete
two-time CONUS table. Resume instructions are in the
[prototype README](https://github.com/andyfangdz/meteogram-js/blob/5f4ae08/scripts/clickhouse-prototype/README.md#full-volume-retry).

Evidence is saved under
[`docs/benchmarks/2026-10-17-clickhouse/full-volume-retry/`](../benchmarks/2026-10-17-clickhouse/full-volume-retry/):
[database measurements](../benchmarks/2026-10-17-clickhouse/full-volume-retry/conus-two-hours-benchmark.json),
[source measurements](../benchmarks/2026-10-17-clickhouse/full-volume-retry/source-measurements.json),
[API checks](../benchmarks/2026-10-17-clickhouse/full-volume-retry/conus-two-hours-api.json),
[estimate arithmetic](../benchmarks/2026-10-17-clickhouse/full-volume-retry/estimates.json),
and [network checks](../benchmarks/2026-10-17-clickhouse/full-volume-retry/retry-network.jsonl).
