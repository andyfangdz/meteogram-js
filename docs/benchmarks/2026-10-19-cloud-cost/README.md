# HRRR cloud cost optimization — 2026-10-19

Cloud coverage remains exact at all 50 model levels and all 1,638,757 retained
CONUS cells per hour. Layer height and pressure are now derived, with the user's
approval, using the existing pressure-level atmosphere and two native anchors.
The stored supplement includes those anchors; they are included in every size
below. The API describes the geometry method and the site labels derived heights.

## Cost target and measurements

The reference is the mean of **eight** stored weather variables, including the
two small surface variables, across the resident 49-hour HRRR run. Compressed
ClickHouse field bytes and all associated weather codebooks are included;
shared coordinates and database identifiers are excluded. The target is
**18,086,243 bytes (17.25 MiB) per variable per hour**. The earlier live phase
trace gives **2.06 seconds per variable** for weather acquisition plus downstream
loading/publication. See [baseline.json](baseline.json).

| Source product / sampled leads | Mean cloud files per hour | Largest added ingestion time |
| ------------------------------ | ------------------------: | ---------------------------: |
| 48-hour, leads 0 / 20 / 48     |  11,532,084 B (11.00 MiB) |                      0.248 s |
| 18-hour, leads 0 / 9 / 18      |  11,227,066 B (10.71 MiB) |                      0.506 s |

These are real full-CONUS reads of **six hours**, covering both source products;
they are not a newly downloaded full 49-hour run. Each timed integrated run
includes the production acquisition, encoding, complete staged readback, hashing,
metadata write and retry-cache pruning. Weather-only and integrated runs use
independent Source instances, fresh staging directories and alternating order.
A weather-only primer equalizes the reference preparation performed by normal
availability discovery; cloud metadata is not primed. Source payloads are not
held in a weather cache. Both branches share at most 16 map jobs on a four-worker
runtime. The regional Icechunk chunk-reference cache is bounded at 8 million.

Cloud extraction overlaps weather extraction. The time above is the change in
**total ingestion elapsed time**, measured against weather-only control runs,
not the cloud branch's standalone duration. Cloud branch durations of roughly
4–6 seconds are recorded in the evidence. The 48-hour mean difference is
−0.106 seconds, which is measurement noise, not a negative processing cost;
the 18-hour mean difference is 0.379 seconds. There is no cloud SQL-load phase.

Every timed pair produced byte-identical weather stages and codebooks. All
**491,627,100 cloud fraction values** and **19,665,084 anchor values** across the
six hours matched the original Float32 bits. Every reconstructed column was
finite and strictly ordered. Evidence retains the snapshot IDs, individual
run timings, file sizes, hashes and full-grid geometry statistics:

- [48-hour product](long-run-source.json)
- [18-hour product](short-run-source.json)

## Representation and geometry accuracy

Coverage uses exact Float32 palettes, constant-level handling and checksummed
256-cell spatial frames with spatial delta compression. Values are neither
quantized nor rounded to integer percentages; unusual fractions and signed zero
are preserved. The second file pair holds exact level-1 pressure and level-50
geopotential height. Acquisition reads 53 maps: 50 fractions, two retained anchors
and a model-top pressure guard, compared with the previous 150-map supplement.

Pressure uses the [NOAA HRRR interfaces](https://rapidrefresh.noaa.gov/faq/HRRR.faq.html)
and [WRF hybrid-coordinate weights](https://github.com/wrf-model/WRF/blob/master/dyn_em/module_initialize_ideal.F),
anchored by native bottom pressure and adjusted using the existing pressure
profile's moisture column. Height interpolates in log pressure, with native top
height bounding the portion above 275 hPa. Native top pressure must agree with
the configured 15 hPa model lid before publication. Geometry is estimated; the
coverage fractions remain exact. Existing pressure-level weather values and the
latitude-dependent geopotential-to-MSL conversion are unchanged.

Across the six full-grid comparisons, within the display's 0–20,000 ft
geopotential range, the 99th percentile of absolute height error is at most
**14.2 m (46.6 ft)**; the largest observed error is **53.74 m (176.3 ft)**.
Errors outside that height range are larger; the per-hour evidence records the
maximum across all 50 levels separately. This is a measured approximation,
not exact native geometry or an accuracy bound for all future forecasts.

## Resident conversion and verification

The conversion command creates immutable cloud replacements while preserving
weather tables, source identity, all forecast hours, codebooks, coordinates and
proofs. It independently compares every retained fraction and both anchors
before publication, then compares them again in the publication gate. All
catalog selector roles are replaced atomically. Previous descriptors retain
the normal ten-minute retirement grace; GC then removes unreferenced files.
Serving accepts only version 2. The original three-field reader is isolated in
the conversion command.

The resident audit at 04:18 UTC checked **94 unique cloud hours**, including the
full 49-hour run, both 19-hour complete runs and the seven available latest-run
hours. Ninety-two original supplements were converted; two hours had already
been ingested in the new format. All catalog roles, weather metadata, table
bindings, forecast hours and cloud source snapshots matched. Independent
conversion and publication checks compared **7,538,282,200 fraction values** and
both exact anchors for every converted cell.

Selected cloud files total **1,086,870,394 bytes (1.012 GiB)**, down from
30,495,641,064 bytes (28.401 GiB). The mean is **11,562,451 bytes (11.03 MiB)**
and the largest hour is **12,469,027 bytes (11.89 MiB)**; every hour is below the
17.25 MiB average-variable target. Replaced files become eligible for normal GC
at 04:26:22 UTC. By 04:31 UTC, normal cleanup had removed all **184 original
cloud files**, totaling **30,473,181,144 unique bytes**. The 92 converted hours
use 1,064,410,474 bytes, giving a net saving of **29,408,770,670 bytes
(27.39 GiB)**. No original cloud payload files remain in the managed store.
See [retained-conversion.json](retained-conversion.json) and
[physical-cleanup.json](physical-cleanup.json).

The native test suite includes database integration checks, exact codec
roundtrips, CRC rejection, conversion proof forgery rejection, binding checks,
geometry validation and retention/ownership checks. Frontend tests, TypeScript,
lint and the production build passed. The browser preview loaded KCDW, Houston
and Denver with all 50 levels and the derived-geometry badge and tooltip; see
[browser-preview.json](browser-preview.json).

The live benchmark is an opt-in ignored Rust test. Its original-coordinate
comparison requires original version-1 hourly files; migrated descriptors are
not interchangeable inputs. Immutable source snapshot IDs are recorded above;
the large original payloads are not committed or retained just for this test.
Current commands and source-cache rules are in [native/README.md](../../../native/README.md).

Version 0.3.0 is installed on the home service. All four models passed authenticated
local and public API checks with the required retained runs. Twelve complete
logical API payloads (three locations and all four HRRR selectors) matched before
and after conversion, including every weather and derived cloud value; only
revision, query duration and wall-clock freshness fields were excluded. See
[native-install-and-api.json](native-install-and-api.json).
