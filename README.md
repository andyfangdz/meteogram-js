## Meteogram (Next.js 16)

An interactive aviation/weather meteogram viewer built with Next.js 16 and React 19. It renders multi-level forecasts from the local native forecast service or Open-Meteo as a time–height cross-section with clouds, temperature isotherms, wind barbs, and pressure lines. Preferences are persisted via cookies and mirrored in the URL for easy sharing.

Key tech: HeroUI for UI, Visx for charts, server actions for data fetch/transform, and App Router dynamic routes.

### Features

- **Multiple weather models**: HRRR, GFS, AIFS, RRFS and other Open-Meteo models (see `src/config/weather.ts` for route names).
- **Flexible locations**: use predefined ICAO sites (e.g., `KFRG`, `KNYC`, `KCDW`) or custom coordinates as `Name@lat,long`.
- **Rich visualization**: cloud columns, temperature lines/isotherms, pressure lines, wind barbs, hover tooltip, and time axis.
- **User preferences**: toggle local time, ceiling highlight, cloud coverage clamp, pressure lines, wind barbs, and isotherm lines. Preferences sync to cookies and URL.
- **Server-side data orchestration**: authenticated native forecasts with Open-Meteo fallback, plus an independent elevation lookup. Source labels identify the actual model, initialization, partial-run progress and estimated cloud coverage.

### Quick start

- **Requirements**: Node 20+ (LTS recommended), Yarn.

```bash
yarn install
yarn dev
# open http://localhost:3000
```

Production build:

```bash
yarn build && yarn start
```

Quality checks:

```bash
yarn lint      # ESLint
yarn ts        # Incremental type checks
yarn format    # Prettier check
yarn format:fix
```

### Using the app

- The root route redirects to a default view based on `src/app/page.tsx` (e.g., `/KCDW/gfs_hrrr`).
- Navigate by URL pattern: `/<location>/<model>`
  - **Predefined locations** (uppercase keys) live in `src/config/weather.ts` under `LOCATIONS`.
  - **Custom location**: `My Spot@40.73443,-73.41639` (use `encodeURIComponent` for spaces). Examples:

```text
/KFRG/gfs_hrrr
/South%20Practice%20Area/ecmwf_ifs025
/MySpot@40.73,-73.42/gfs_seamless
```

- Use the top navigation to select location/model and toggle visualization preferences. The URL and a cookie reflect your choices for easy sharing and persistence.

### Architecture overview

- **App Router**: dynamic route at `src/app/[location]/[model]/page.tsx` resolves `params`/`searchParams` (Promises in Next.js 16), validates `model`, fetches data server‑side, and hydrates the client UI wrapped by `ClientWrapper`.
- **Data fetch path**: `getWeatherData` in `src/app/actions/weather.ts` orchestrates:
  - `fetchWeatherDataAction` (Open‑Meteo forecast; injects per‑model variable lists from `ModelConfig.getAllVariables()`)
  - `fetchElevationAction` (elevation lookup; meters → feet)
  - Tries the configured native provider for supported CONUS routes before Open-Meteo.
  - Returns `{ data, timestamp, elevationFt, source }` for the client.
- **Transform & types**: `transformWeatherData` (in `src/utils/weather.ts`) converts API responses into `CloudColumn[]`, computing MSL tops/bottoms and filtering non‑finite values. Shared types live in `src/types/weather.ts`.
- **Visualization stack**: `Meteogram` composes `CloudColumns`, `WeatherLines`, `PressureLines`, `HoverIndicators`, and `MeteogramTooltip` with Visx scales. `useMeteogramScales` handles domains and optional cloud opacity clamping.
- **Preferences**: `PreferencesContext` merges defaults → cookies → URL. `VisualizationPreferencesComponent` wires toggles that write directly back to context.

### Weather models & configuration

- Models and their cadence/levels are defined in `MODEL_CONFIGS` within `src/config/weather.ts`. Each `ModelConfig` sets:
  - `forecastDataKey`, `stepKey`, `stepSize` (minutely/hourly cadence and extent)
  - `hpaLevels` (vertical structure), `windBarbStep`, `windBarbPressureLevelStep`
  - `maxIsothermStepDistance` (isotherm line continuity)

To add or tune a model, extend `MODEL_CONFIGS` and keep variable lists aligned with `transformWeatherData` indexing.

### Locations

- Predefined sites live in `LOCATIONS` in `src/config/weather.ts`.
- Use uppercase keys for airports and readable names for practice areas.
- Routes for names with spaces must be URL‑encoded (e.g., `South%20Practice%20Area`).
- Custom coordinates are accepted via `Name@lat,long`.

### Extending preferences

When adding a new visualization preference, update all of the following to keep cookie/URL parity and UI toggles in sync:

- `src/config/preferences.ts`
- `src/context/PreferencesContext.tsx` (serializer/parser)
- Any query‑string helpers (e.g., `src/utils/params.ts`)
- The nav toggles in `src/app/components/visualization-preferences.tsx`

### Units & conventions

- Heights are in feet above mean sea level (MSL). Pressure levels are in hPa. Wind barbs and other conversions reuse helpers in `src/utils/meteogram.ts` (e.g., `kmhToKnots`, `hPaToInHg`).
- The meteogram expects the data sorted by descending hPa.

### Deployment

- Optimized for Vercel. See `vercel.json` for build/dev/install commands. This app uses standard Node APIs only (no filesystem writes) to remain edge‑compatible with server actions.

To enable native forecasts, set these **server-only** environment variables in
Vercel and redeploy:

```text
METEOGRAM_API_URL=https://weather-api.andyfang.app
METEOGRAM_API_TOKEN=<same secret as the native service api-token file>
```

Do not prefix them with `NEXT_PUBLIC_`. The browser calls Next.js server actions;
it never receives the API token or calls the home API directly. For local
development, an ignored `.env.local` can use `http://127.0.0.1:18080`.

`best_match` and `gfs_hrrr` select native HRRR, `gfs_seamless` selects native GFS,
`ecmwf_aifs025_single` selects native AIFS, and `rrfs` selects native RRFS. Native
coverage is 24–50°N, 125–66°W, subject to each model's grid. Other models and
locations use Open-Meteo. A missing/stale/invalid native forecast or a five-second
timeout falls back to Open-Meteo; RRFS uses GFS as its backup and labels that
actual source. Leaving the API URL unset preserves the Open-Meteo path.

The adapter preserves native pressure levels and forecast times, including
six-hour AIFS and the three-hour GFS tail. HRRR/RRFS grid winds are rotated to
true bearings. GFS supplies cloud fraction; HRRR, RRFS and AIFS clouds are
humidity estimates, identified above the chart. See
[native service setup](native/README.md) for authentication and Cloudflare Tunnel.

### Acknowledgments

- Forecast data provided by [Open‑Meteo](https://open-meteo.com/). Elevation data via Open‑Meteo Elevation API.
- Native NOAA/ECMWF forecast data is distributed through [Dynamical](https://dynamical.org/).

### License

No license file is included. Add one if you plan to distribute.
