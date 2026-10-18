# Native forecast site integration — 2026-10-18

The site now has a tested native forecast adapter and authenticated API path.
The public Cloudflare path is verified, including all four charts. The
new application commit is deployed to production. Production native
activation still awaits the environment correction described below.

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

Commit `9684287` built successfully in the Vercel preview deployment
`dpl_3YXtuQ1SDNUYcVecth4cjjU4awo3`. The user subsequently reported configuring
the server environment variables. The connector cannot list their metadata
(403), so the values/targets were not independently inspected. Deployment
creation and protected-preview access were rejected with
`MCP tool call requires approval, but approval policy is never`. The user promoted the tested application commit: production deployment
`dpl_GDC6C4Gb7oRsuffbhojbodKwwbaC` is READY at `meteogram.vercel.app` and
`meteogram.flyingclub.org`, running commit `9684287`.

The first production browser check stopped at HRRR: the page rendered
Open-Meteo rather than native data. A separate public response inspection
confirmed `provider: open-meteo, model: gfs_hrrr, fallback: false`. Given this
commit's source selection, that indicates `METEOGRAM_API_URL` is absent or
empty in the production runtime.
No warning/error/fatal runtime logs were returned for this deployment in the
checked ten-minute window. The user was asked to confirm **both** server
variables target Production and redeploy with the corrected variables. Hosted
native charts remain unverified until that correction. See
`production-source-check.json` for the response evidence.

`meteogram-native.service` and `meteogram-tunnel.service` are installed in the
user unit search path with `default.target.wants` links; the host already has
user lingering enabled. Unit verification passed, apart from an unrelated
system-provided `spice-vdagent` warning. The session cannot connect to the user
service-manager bus, so the current processes remain direct instances and a
managed restart/reboot has not been verified. Stop direct instances before
starting the units to avoid occupied loopback ports.
