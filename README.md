## Meteogram (Next.js 15)

An interactive aviation/weather meteogram viewer built with Next.js 15 and React 19. It fetches multi-level forecast data from the Open‑Meteo API and renders a compact time–height cross‑section with clouds, temperature isotherms, wind barbs, and pressure lines. Preferences are persisted via cookies and mirrored in the URL for easy sharing.

Key tech: HeroUI for UI, Visx for charts, server actions for data fetch/transform, and App Router dynamic routes.

### Features
- **Multiple weather models**: `best_match`, `gfs_seamless`, `gfs_hrrr`, `ecmwf_ifs025`, `ecmwf_aifs025`, `gem_seamless`, `gem_hrdps_continental` (see `src/config/weather.ts`).
- **Flexible locations**: use predefined ICAO sites (e.g., `KFRG`, `KNYC`, `KCDW`) or custom coordinates as `Name@lat,long`.
- **Rich visualization**: cloud columns, temperature lines/isotherms, pressure lines, wind barbs, hover tooltip, and time axis.
- **User preferences**: toggle local time, ceiling highlight, cloud coverage clamp, pressure lines, wind barbs, and isotherm lines. Preferences sync to cookies and URL.
- **Server‑side data orchestration**: Open‑Meteo forecast + elevation lookup combined and transformed server‑side for fast client hydration.

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
- **App Router**: dynamic route at `src/app/[location]/[model]/page.tsx` resolves `params`/`searchParams` (Promises in Next.js 15), validates `model`, fetches data server‑side, and hydrates the client UI wrapped by `ClientWrapper`.
- **Data fetch path**: `getWeatherData` in `src/app/actions/weather.ts` orchestrates:
  - `fetchWeatherDataAction` (Open‑Meteo forecast; injects per‑model variable lists from `ModelConfig.getAllVariables()`)
  - `fetchElevationAction` (elevation lookup; meters → feet)
  - Returns `{ data, timestamp, elevationFt }` for the client.
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

### Acknowledgments
- Forecast data provided by [Open‑Meteo](https://open-meteo.com/). Elevation data via Open‑Meteo Elevation API.

### License
No license file is included. Add one if you plan to distribute.
