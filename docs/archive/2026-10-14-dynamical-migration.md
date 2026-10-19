# Dynamical migration investigation

> Historical record. Measurements, commands and deployment status below apply to
> the recorded version. See the [current native service guide](../../native/README.md)
> for supported formats, commands and deployed behavior.

Verified against the live Dynamical catalog on 2026-10-14. The requested scope is
to replace every existing model that Dynamical can support. The app still uses
Open-Meteo; this investigation adds a runnable access/performance probe, not a
provider switch.

## Model coverage

| Current model           | Dynamical product                                                    | Migration status                                                                                                      |
| ----------------------- | -------------------------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------- |
| `gfs_seamless`          | `noaa-gfs-forecast-virtual`                                          | GFS profiles available. This replaces Open-Meteo's seamless product with individual GFS runs, not an identical blend. |
| `gfs_hrrr`              | `noaa-hrrr-forecast-48-hour-virtual`                                 | HRRR profiles available over CONUS; native hourly steps and 6-hourly extended runs.                                   |
| `ecmwf_aifs025_single`  | `ecmwf-aifs-single-forecast-virtual`                                 | AIFS profiles available, with sparse pressure levels and native 6-hourly steps.                                       |
| `ecmwf_ifs`             | No matching deterministic IFS profile product in the current catalog | Keep Open-Meteo. The IFS ensemble product is a different model product and lacks the required full profile.           |
| `gem_hrdps_continental` | `eccc-hrdps-forecast`                                                | Keep Open-Meteo: the current Dynamical product contains surface fields, not full vertical profiles.                   |
| `gem_seamless`          | No equivalent seamless product                                       | Keep Open-Meteo.                                                                                                      |
| `best_match`            | No equivalent automatic blend                                        | Keep Open-Meteo unless a new, explicitly defined model-selection policy replaces it.                                  |

The materialized GFS and HRRR products are optimized for time series but contain
surface fields rather than the profiles needed by this app. AIFS's materialized
product has only a few upper-air fields. The virtual products contain the needed
vertical data.

## Access and measured cost

Current catalog assets are **Zarr v3 in Icechunk v2 repositories**, with virtual
chunks pointing at public GRIB messages. They are not ordinary HTTP Zarr
directories that can be substituted for the Open-Meteo URL. The tested stack was
Python 3.12, Icechunk 2.3.0, Gribberish 1.8.4, Xarray 2026.9.0 and Zarr 3.4.1.
No API key was needed. Read the STAC collection to resolve asset URLs and
authorize its public virtual-chunk containers anonymously.

Live scalar temperature reads succeeded for GFS, HRRR, and AIFS, using the
2026-10-14 12Z run. Dataset opening took approximately 7–18 seconds; a subsequent
single-level temperature read took 1.4–1.8 seconds in this workspace.

A complete GFS column at the nearest grid point to KCDW, lead hour 6, with 30
levels and six variables took **20.21 seconds**, excluding dataset opening.
All 180 values were finite. This used two parallel variable reads and Zarr
asynchronous concurrency four. These are one-run observations through this
workspace's network proxy, not production latency guarantees.

The repository probe was then checked against all three products: a full GFS
column took 20.924 seconds, and one-level HRRR/AIFS samples across all required
fields took 4.366/4.267 seconds. All sampled values were finite. Those checks ran
concurrently, so the timings should not be used to rank the products.

The limiting factor is chunk geometry: one time, one level, and the entire
spatial grid. Selecting a single location still decodes that map.

| Product | Decoded spatial chunk | Representative forecast                               | Profile chunks | Aggregate decoded volume |
| ------- | --------------------- | ----------------------------------------------------- | -------------- | ------------------------ |
| GFS     | 721 × 1440 × float64  | 137 native times over 0–168h × 30 levels × 6 fields   | 24,660         | 190.8 GiB                |
| HRRR    | 1059 × 1799 × float64 | 40 hourly times × 30 levels × 6 fields                | 7,200          | 102.2 GiB                |
| AIFS    | 721 × 1440 × float64  | 29 six-hourly times over 0–168h × 8 levels × 5 fields | 1,160          | 9.0 GiB                  |

These are metadata-derived **aggregate decoded volumes**, not network download
sizes, peak RAM, or measured full-forecast timings. They exclude surface fields.
The native AIFS levels within the app's current 1000–275 hPa bounds are 1000,
925, 850, 700, 600, 500, 400 and 300 hPa. Interpolating to more levels or times
would not add model information.

## Required field transformations

- GFS: use `cloud_cover`, `geopotential_height`, `temperature`, `wind_u`,
  `wind_v`, and `relative_humidity` from `pressure_level`; derive dew point.
- HRRR: use temperature, height, U/V winds, relative humidity and dew point from
  `pressure_level`. This group has no cloud fraction. Estimate cloud cover from
  humidity and identify it as an estimate, or separately sample model-level
  cloud fraction and interpolate using pressure. Cloud mixing ratio is not cloud
  coverage and cannot be substituted directly.
- AIFS: derive dew point and humidity from specific humidity, temperature and
  pressure; estimate cloud cover. The pressure-level group has no cloud fraction.
- Read 2 m temperature and dew point from the root group using the same snapshot
  and initialization time. Preserve the existing independent terrain-elevation
  lookup, or explicitly distinguish model orography from terrain elevation.
- Temperatures in these stores are already Celsius: the codec applies the Kelvin
  offset. Wind components are m/s; the existing renderer expects km/h and a
  meteorological _from_ direction.
- HRRR U/V are grid-relative. Select projected coordinates using `spatial_ref`,
  reject points outside its domain, and rotate winds to east/north before
  computing direction. The grid convergence near KCDW is approximately 14.46°.
- Keep the existing latitude-dependent WGS84 geopotential-to-geometric-height
  conversion, descending-pressure order, finite-value filtering, layer bounds,
  and stability calculations.
- Use UTC `init_time + lead_time` timestamps, native cadence, and actual available
  pressure levels. Check run completeness; the latest initialization can still
  be ingesting. Missing chunks must never become clear-sky or zero-wind data.
- Add source attribution and distinguish model initialization from fetch time.
  Dynamical licenses these datasets under CC BY 4.0.

## Architecture needed for the switch

Do not perform complete virtual-profile reads in the browser or on every Next.js
server-action request. A Python reader alone does not fix the cold-read cost.
It needs background extraction and a durable cache:

1. Resolve the catalog and pin a complete model run/snapshot.
2. Extract profiles in a worker with bounded decoding concurrency. Batch known
   locations so each decoded map can serve several sites in one pass.
3. Publish small normalized point profiles keyed by dataset, run and grid point.
   Prewarm common locations; custom locations need an asynchronous loading path.
4. Have `getWeatherData` read cached profiles for the three supported model
   routes. Keep the other routes on Open-Meteo and show the actual data source.
5. Reuse a provider-neutral profile-to-`CloudColumn[]` transformation so chart
   geometry and derived meteorology remain consistent.

For arbitrary locations with consistently low cold latency, a separate process
must rechunk vertical data into a time-series-friendly store, with storage and
refresh costs assessed before rollout. Memory-only caching in a short-lived
function is insufficient.

Update, 2026-10-16: [the cost-focused design](2026-10-16-dynamical-cost-design.md)
recommends bounded scheduled extraction, static point files and an Open-Meteo
fallback, with no continuously running backend.

Update, 2026-10-17: the user selected arbitrary locations over the continental US
and a home computer is available. The
[regional serving design](2026-10-17-dynamical-arbitrary-locations.md) replaces
the static-only proposal with a retained regional grid and a home query API;
ClickHouse and point-oriented materialized Zarr are the store candidates.

## Reproduce the bounded probe

From the repository root, with `uv` installed:

```bash
uv run scripts/probe-dynamical.py --model gfs
uv run scripts/probe-dynamical.py --model hrrr --levels 1
uv run scripts/probe-dynamical.py --model aifs --levels 1
```

The first command reads one complete pressure column, not a seven-day forecast.
The others read one level across the required variables. Optional `--latitude`,
`--longitude` and `--lead-hour` select a different point/time. Output records the
run, selected grid point, units, finite counts, timings and chunk-cost estimates.
With `--levels`, cost estimates reflect that smaller sampled level count. The
probe deliberately does not assert that the latest run is complete.

## Sources

- [Dynamical catalog](https://dynamical.org/catalog/)
- [GFS virtual forecast](https://dynamical.org/catalog/noaa-gfs-forecast-virtual)
- [HRRR virtual 48-hour forecast](https://dynamical.org/catalog/noaa-hrrr-forecast-48-hour-virtual)
- [AIFS virtual forecast](https://dynamical.org/catalog/ecmwf-aifs-single-forecast-virtual)
- [HRDPS forecast](https://dynamical.org/catalog/eccc-hrdps-forecast)
- [IFS ensemble forecast](https://dynamical.org/catalog/ecmwf-ifs-ens-forecast-15-day-0-25-degree)
- [STAC catalog](https://stac.dynamical.org/catalog.json)

Source attribution: NOAA NWS NCEP GFS/HRRR data and ECMWF AIFS data processed by
dynamical.org from the respective open-data archives.
