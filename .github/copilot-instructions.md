**Project Snapshot**

- Next.js 15 App Router app at `src/app/[location]/[model]/page.tsx`; resolves `params`/`searchParams` Promises, validates `model`, and fetches data server-side before hydrating HeroUI-driven UI.
- Weather data comes from Open-Meteo via `openmeteo` SDK; `MODEL_CONFIGS` in `src/config/weather.ts` determines variables/intervals per model and must stay in sync with `transformWeatherData` indexing.
- Types in `src/types/weather.ts` standardize `CloudColumn`, `CloudCell`, and `VisualizationPreferences` shared between server actions and client rendering.
- HeroUI components provide layout/navigation while Visx (`Meteogram`, `CloudColumns`, `TimeAxis`) handles visualization layers.

**Data Fetch Path**

- `getWeatherData` in `src/app/actions/weather.ts` orchestrates `fetchWeatherDataAction` (Open-Meteo forecast) and `fetchElevationAction`, returning `{data,timestamp,elevationFt}` for the client.
- `fetchWeatherDataAction` accepts ICAO keys or `Name@lat,long` custom strings, derives coordinates, and injects `ModelConfig.getAllVariables()` into the query; keep this logic when adding models.
- `transformWeatherData` in `src/utils/weather.ts` converts the SDK response into sorted `CloudColumn.cloud` entries and computes `mslFtTop/Bottom`; guardrails filter non-finite values.
- Routes are `force-dynamic`, but `generateStaticParams` prebuilds known `LOCATIONS`; adjust both when expanding airports/models.

**Client Runtime**

- `ClientWrapper` (client component) wraps children in `PreferencesProvider`, keeps weather state, and triggers background refreshes every 60s with the same server action.
- Location/model changes go through `next/navigation` router pushes while appending serialized preferences via `serializeVisualizationPreferences`.
- `PreferencesContext` in `src/context/PreferencesContext.tsx` merges defaults → server cookies → URL params; on cookie read failure it retries client-side before syncing URL + cookie.
- `VisualizationPreferencesComponent` wires `NavWrapper` + `MeteogramWrapper`; the nav toggles write straight back to context, so any new preference must be added to both the context serializer and nav switches.

**Visualization Stack**

- `Meteogram` composes `CloudColumns`, `WeatherLines`, `PressureLines`, `HoverIndicators`, and `MeteogramTooltip`; it expects `weatherData` sorted by descending `hpa` and uses Visx scales for axes.
- `useMeteogramScales` clamps cloud opacity to 50% when the preference is enabled; changing the y-domain or clamps requires updates here and in dependent components.
- Temperature/isotherm overlays rely on `findIsothermPoints` and `findFreezingLevels` in `src/utils/meteogram.ts`, which build high-res grids with `marching-squares`; keep `maxIsothermStepDistance` aligned with the model config to avoid broken lines.
- Tooltip, wind barb, and formatting helpers live in `src/app/components/*.tsx` and `src/utils/meteogram.ts`; reuse them instead of reimplementing conversions (`kmhToKnots`, `hPaToInHg`, `formatNumber`).

**Weather Model & Location Config**

- `MODEL_CONFIGS` encodes per-model cadence (`forecastDataKey`, `stepKey`, `stepSize`) and vertical structure (`hpaLevels`, `windBarbStep`, `windBarbPressureLevelStep`); extend this class rather than branching in rendering code.
- Predefined sites live in `LOCATIONS`; use uppercase keys for airports and readable names for practice areas, and always route with `encodeURIComponent` for spaces.
- `DEFAULT_PARAMS`, `FEET_PER_METER`, and geopotential→MSL math in `transformWeatherData` keep units consistent; if you change units, update the tooltip formatters and ground elevation rendering.
- Adding a new preference demands edits in `src/config/preferences.ts`, the context serializer/parser, and any query-string helpers to maintain cookie/URL parity.

**Developer Workflow**

- Run `yarn dev` for local work; `yarn build && yarn start` reproduces the production pipeline Vercel will execute.
- `yarn lint`, `yarn ts`, `yarn format`, and `yarn format:fix` cover ESLint, incremental type checks, and Prettier; CI expects clean runs before merging.
- Server actions log to the console on both server and client; keep logs while debugging cookie fallbacks or API calls but remove noisy ones before shipping.
- Vercel deployment uses `vercel.json`; stay within standard Node APIs (no filesystem writes) so server actions remain edge-compatible.
