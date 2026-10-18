# Native forecast site integration — 2026-10-18

The native forecast integration is live at `meteogram.vercel.app` and
`meteogram.flyingclub.org`, using `weather-api.andyfang.app`. All four native
models passed production browser checks. The home API, database, updater and
tunnel now run under enabled user services. No host reboot was performed.

## Request path

The browser renders Next.js data and refreshes through server actions. The server
uses `METEOGRAM_API_URL` and `METEOGRAM_API_TOKEN` to request a point forecast
through a named Cloudflare Tunnel to loopback port 18080 on the home host.
ClickHouse ports and tunnel metrics remain on loopback. An API token was created
outside Git in `~/.local/share/meteogram-native/api-token`, mode 0600. All native
routes require bearer authentication with that configuration; malformed token
files fail startup. The token is absent from the browser's built assets.

`best_match` and `gfs_hrrr` select native HRRR, `gfs_seamless` selects GFS,
`ecmwf_aifs025_single` selects AIFS, and the new `rrfs` route selects RRFS.
Other routes and locations outside 24–50°N, 125–66°W use Open-Meteo. Native grid
domain checks remain authoritative inside that bounding box. Native fetches
have a five-second limit, disable redirects and caching, and validate identity,
schema, pressure levels, expected/available/missing hours, units, wind frame,
freshness and usable forecast horizon. Failure selects Open-Meteo; RRFS uses GFS
as its explicitly labeled backup. Elevation is fetched independently with a
four-second limit and daily cache.

## Meteorological normalization

The adapter preserves native pressure levels and lead times. AIFS keeps its
eight pressure levels and six-hour cells; GFS keeps hourly cells through hour
120 and three-hour cells afterward. Regional cells remain hourly. Missing leads
are absent, and cell width uses the native cadence rather than the distance to
the next available lead. No missing layers or hours are filled with zero.

The existing latitude-dependent WGS84 height conversion and shared column
geometry/lapse-rate calculations serve both providers. Regional wind vectors
rotate from their actual spherical Lambert grid into true east/north components
before converting to meteorological from direction and km/h. The API supplies
the convergence at the selected cell; GFS/AIFS rotation is zero.

GFS uses native cloud fraction and derives dew point from relative humidity.
HRRR/RRFS retain native dew point and estimate cloud cover from humidity. AIFS
converts specific humidity to mixed-phase RH with Murphy–Koop saturation
pressures and ECMWF weighting, then estimates cloud cover and derives dew point.
The pressure-dependent Sundqvist cloud estimate follows
[Open-Meteo's implementation](https://github.com/open-meteo/open-meteo/blob/main/Sources/App/Helper/Meteorology.swift).
The chart labels these estimates and shows actual provider/model, UTC run time
and complete/partial progress.

## Verified locally

Evidence is under `docs/benchmarks/2026-10-18-native-site/`:

- All 79 TypeScript tests passed, including adapter physics/schema tests and
  native-provider authentication, timeout, fallback and coordinate tests.
- Type checking and the Next.js production build passed. ESLint had no errors;
  its one warning concerns the existing Yarn release file.
- All 51 Rust tests passed, including route authentication, invalid token-file
  rejection and Lambert wind-rotation sign. Rust formatting and Clippy passed.
- The installed authenticated service passed 36 local HTTP/profile checks.
  This was an HTTP verification, not a repeated full database validation.
- Headless Chromium rendered HRRR, GFS, AIFS and RRFS charts with native source
  labels and nonempty cloud cells, without page errors. AIFS was visually
  inspected. A second check passed model-menu navigation, manual refresh,
  preference persistence after reload, arbitrary CONUS coordinates and London
  fallback to Open-Meteo. The browser made no direct native API requests.
- The built client assets were scanned for the API token; no matches were found.

## External activation status

The active hostname is **weather-api.andyfang.app**. The user selected this
single-label hostname after the original two-label name lacked Cloudflare
certificate coverage. The replacement named tunnel is `meteogram-weather-api`
(`f65d971f-8364-4f2f-9c8e-215c53c04d14`), with credentials stored outside Git and
four ready HTTP/2 connections. Its local metrics listener is 127.0.0.1:20243.
The original session-created tunnel and its obsolete DNS record were deleted;
unrelated tunnels were left alone.

Public HTTPS verification passed with ordinary certificate validation:
unauthenticated health returned 401, and authenticated HRRR, GFS, AIFS and RRFS
forecasts each returned 200 with usable data. The first individual requests took
459, 1100, 95 and 2151 ms respectively; these are observations, not latency
percentiles or a service guarantee. A local Next.js production server configured
with the public URL rendered all four native charts in Chromium without page
errors. See `public-tunnel.json` and `browser-cloudflare.json` in the evidence
directory. This checks browser → Next.js → public Cloudflare TLS → home API →
ClickHouse → normalized chart; it does not establish the deployed Vercel flow.

## Production verification and managed services

Production deployment `dpl_3b88RLf36s3nW2XEzyaQGLDGSypK` is READY, running
application commit `9684287`. Later branch commits update documentation and
evidence only. The user configured both server variables for Production and
redeployed through Vercel. The first production deployment had omitted the
native URL; `initial-production-source-check.json` records that historical
response. The current production evidence supersedes it.

`browser-production.json` confirms HRRR, GFS, AIFS and RRFS each returned HTTP
200 and rendered native chart cells with Dynamical source labels, without
browser page errors. The checked HRRR, GFS and AIFS runs were complete; RRFS
reported its current partial progress. `browser-production-interactions.json`
confirms manual refresh through server-action POSTs on both public domains and
a custom CONUS coordinate on the flyingclub domain. No browser receives the
native API token.

After the session's local permissions changed, access to the user service
manager became available. Both units were enabled through `systemctl --user`.
The direct native supervisor was stopped through its durable stop request,
then `meteogram-native.service` was started. The direct tunnel wrapper was
stopped before starting `meteogram-tunnel.service`. The old disconnected tunnel
connector and temporary Next.js verification servers were stopped.

`managed-service-status.json` records both units active/running and enabled,
with user lingering enabled and all three native children (API, ClickHouse and
incremental updater) running. The managed service passed 36 HTTP/profile checks;
the tunnel had four ready connections, and authenticated public health showed
all four models available. A further production RRFS browser check after the
service transition passed with native data and no page errors. Unit startup and
publication continuity were checked; a host reboot was not tested. Current
operations use `systemctl --user status|restart|stop` for these units.

The final runtime scan returned no native-route warnings/errors. It did return
six error records for two requests to the separate Open-Meteo-only
`/KCDW/ecmwf_ifs` route, reporting `Too many concurrent requests`. That route
continues to depend on Open-Meteo and was not changed by this integration.
`production-runtime-scan.json` records the window and request IDs; do not
characterize the complete production log as error-free.
