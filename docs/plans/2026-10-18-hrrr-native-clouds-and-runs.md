# Native HRRR clouds and initialization overlays

The home service now retains HRRR's actual 50 model-level cloud fractions,
geopotential heights and pressures for both its 18-hour and 48-hour forecasts.
Cloud shading uses those model heights; the existing 30 pressure levels continue
to supply temperature, wind and thermodynamics. No forecast values are resampled
to 15-minute timesteps.

The site now overlays the newest validated HRRR intervals on the latest complete
48-hour HRRR run. While a new short run is arriving, the previous complete short
run supplies its missing intervals. GFS, AIFS and RRFS likewise overlay the newest
run on the latest complete run. Every interval keeps its supplying initialization.
Colored run strips and vertical handoff lines identify changes; labels avoid
collisions, and tooltips show initialization and any original coarse sample time.

## Data and source integrity

Native clouds are separate, durable, checksummed Float32 files in each immutable
hourly descriptor, with shape `[1638757, 50, 3]`. The three fields are
`fraction_of_cloud_cover` (0–1), `geopotential_height` (m) and `pressure` (Pa).
All retained values receive finite/range/vertical-order checks and a complete
bit-exact staged readback before publication. Native point reads verify frame
checksums. The descriptor's metadata hash binds the index hash, file length,
source snapshot, initialization, forecast hour and exact retained grid.

The first attempted backfill pinned an older pressure snapshot and found all
150 cloud references absent. Current source snapshots had the native references
for the same forecast. Cloud acquisition therefore pins its own immutable
snapshot, preserving the corresponding `pressure_snapshot_id` separately.
Dataset, initialization, hour and grid must match exactly. Metadata declaring a
variable is not proof that its payload is present.

Cloud-only upgrades clone hourly and parent descriptors. Their publication gate
requires unchanged pressure tables, hours, metadata, codebooks, coordinates and
validation proofs. An absent cloud supplement may be added; existing native
clouds cannot be replaced. Existing retention and catalog locks protect the
cloud files together with their pressure hour. No new database table is needed
for these supplements.

Authoritative source schemas:
[Dynamical 18-hour collection](https://stac.dynamical.org/noaa-hrrr-forecast-18-hour-virtual/collection.json),
[Dynamical 48-hour collection](https://stac.dynamical.org/noaa-hrrr-forecast-48-hour-virtual/collection.json).

## Completed measurements

These are actual full-CONUS cloud backfills with pressure data already resident.
They include cloud acquisition, all-value validation/readback and publication.
They are not complete pressure-plus-cloud refresh benchmarks.

| Run            | Forecast times | Cloud Float32 values checked |        Compressed cloud data | Wall time | Peak client RSS |
| -------------- | -------------: | ---------------------------: | ---------------------------: | --------: | --------------: |
| HRRR 20Z short |             19 |                4,670,457,450 |   6,248,071,133 B (5.82 GiB) |  3m27.47s |        1.55 GiB |
| HRRR 18Z full  |             49 |               12,044,863,950 | 16,328,435,289 B (15.21 GiB) |  9m32.89s |        1.61 GiB |

Each run retains all 1,638,757 CONUS cells and all 50 cloud levels. Data byte
counts exclude small JSON indexes and descriptors. Downloaded GRIB bytes were
4,231,500,399 and 11,137,134,083 respectively.

A separate, isolated fresh-hour test exercised the normal updater through
pressure/cloud acquisition, ClickHouse loading, complete validation, attachment
and publication: 32.35 seconds, peak client RSS 2.61 GiB. Its native cloud files
survived staging cleanup and were served successfully. That is a one-hour test,
not a completed fresh 49-hour pressure refresh.

After backfill and service restart, pressure profiles at KCDW, Seattle, Denver
and Houston matched their saved before-backfill responses for both runs: every
pressure field, level, hour, unit, resolved grid point and wind rotation was
unchanged. Twenty warm loopback requests per selector measured:

| Complete profile                              |      p50 |      p95 |  Maximum |
| --------------------------------------------- | -------: | -------: | -------: |
| 19 forecast times plus 50 native cloud levels | 303.8 ms | 324.4 ms | 324.4 ms |
| 49 forecast times plus 50 native cloud levels | 727.6 ms | 767.5 ms | 772.1 ms |

These observations are not a WAN SLA or an ingestion-load tail-latency guarantee.

## Run selection and rendering

Requests for the required retained runs execute concurrently using existing
server-only bearer authentication and the five-second per-request timeout.
The merge chooses the newest available initialization for each native interval.
Missing intervals retain the complete base. If a coarse GFS interval is split
by newer data, both pieces retain the exact source values and original sample
time; no forecast values are interpolated. Same-initialization responses are
deduplicated while preserving the longest horizon metadata.

Actual browser verification used a production Next.js build, through the public
HTTPS Cloudflare endpoint to the managed home API and local stores. It verified
all four native model routes, the full HRRR horizon, native cloud rectangles,
initialization markers, initialization-aware tooltips and manual refresh.
A newly arriving 21Z partial run exercised the three-layer case in real data:
21Z intervals, then the complete 20Z short run, then the 18Z 48-hour tail.
The partial GFS 18Z run also exercised repeated gaps filled from its complete
12Z base. Screenshots were inspected for label collisions and readability.

Open-Meteo-only routes explicitly report initialization unavailable. Their
standard forecast response does not identify the initialization supplying each
interval. The separate model-updates API does not establish that attribution;
its documentation warns that its times do not directly map to forecast results.
See [Open-Meteo model metadata documentation](https://open-meteo.com/en/docs/model-updates).
No initialization is inferred from response computation time.

## Validation and deployment status

- 102 frontend tests passed, including full/partial overlays, missing intervals,
  coarse sample preservation, all-model markers, dense label collisions and
  native cloud rendering/provenance.
- 57 Rust tests passed, including real ClickHouse integration, native cloud
  checksums, shape/provenance validation, immutable cloud upgrades and retention.
- TypeScript, production Next.js build, Clippy with warnings denied and formatting
  checks passed. ESLint reported zero errors and one pre-existing warning in
  the bundled Yarn release.
- The tested native binary is installed and the managed native/tunnel units are
  active and enabled. Public access requires bearer authentication.
- The new site code is validated locally through the real public API. Production
  Vercel promotion remains a separate final step; no environment changes are
  required. This report does not claim the new frontend is already deployed.

Evidence is under
[`docs/benchmarks/2026-10-18-hrrr-native-clouds/`](../benchmarks/2026-10-18-hrrr-native-clouds/).
Large cloud files, source caches and executable binaries remain outside Git.
