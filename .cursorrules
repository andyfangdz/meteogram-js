# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

This is a Next.js 15 application that visualizes aviation weather data using meteograms. It displays cloud coverage, temperature, wind, and other atmospheric data at different pressure levels using data from the Open-Meteo API. The application is designed for pilots to assess weather conditions at various altitudes.

## Commands

### Development
```bash
yarn dev          # Start development server on localhost:3000
yarn build        # Build for production
yarn start        # Start production server
```

### Code Quality
```bash
yarn lint         # Run ESLint
yarn ts           # Type check without emitting files (incremental)
yarn format       # Check formatting with Prettier
yarn format:fix   # Fix formatting issues
```

## Architecture

### Routing & Data Flow

The app uses Next.js 15 App Router with dynamic routes:
- Root (`/`) redirects to `/KFRG/gfs_hrrr` (default location and model)
- Main route: `/[location]/[model]` where location can be:
  - Predefined airport codes (e.g., KFRG, KNYC, KCDW) from `src/config/weather.ts`
  - Custom coordinates in format: `Name@latitude,longitude` (e.g., `Custom@40.5,-73.5`)

**Key Data Flow:**
1. Server-side page (`src/app/[location]/[model]/page.tsx`):
   - Validates model (must be in MODEL_NAMES)
   - Fetches weather data via `getWeatherData()` server action
   - Loads user preferences from cookies or URL params
   - Passes initial data to client components
2. Server actions (`src/app/actions/weather.ts`):
   - `fetchWeatherDataAction()`: Fetches raw data from Open-Meteo API
   - `fetchElevationAction()`: Gets ground elevation for location
   - `getWeatherData()`: Orchestrates data fetching and transformation
3. Data transformation (`src/utils/weather.ts`):
   - `transformWeatherData()`: Converts Open-Meteo response into CloudColumn format
   - Calculates MSL heights from geopotential heights
   - Filters invalid data points

### Weather Models

Configured in `src/config/weather.ts` with the `ModelConfig` class:
- Each model has different forecast resolution (hourly vs 15-minute intervals)
- Different pressure levels (hPa): DEFAULT_HPA_LEVELS (1000-250 by 25) or GEM_HPA_LEVELS
- Model-specific parameters:
  - `forecastDataKey`: "hourly" or "minutely15"
  - `windBarbStep`: How often to show wind barbs (e.g., every 3 hours)
  - `windBarbPressureLevelStep`: Wind barb frequency by altitude
  - `maxIsothermStepDistance`: Max gap for temperature line continuity

Available models: best_match, gfs_seamless, gfs_hrrr, ecmwf_ifs025, ecmwf_aifs025, gem_seamless, gem_hrdps_continental

### State Management

**Preferences Context** (`src/context/PreferencesContext.tsx`):
- Manages visualization preferences (time zone, cloud coverage display, overlays)
- Three-tier persistence strategy:
  1. URL search params (highest priority, shareable)
  2. Cookies (persistent across sessions)
  3. Defaults from `src/config/preferences.ts`
- Server reads URL params → merges with cookies → client can read cookies directly if server fails
- Changes sync to both URL and cookies

**Preferences:**
- `useLocalTime`: Display times in local or UTC
- `highlightCeilingCoverage`: Highlight cloud ceilings
- `clampCloudCoverageAt50Pct`: Clamp opacity at 50% cloud coverage
- `showPressureLines`: Show isobaric lines
- `showWindBarbs`: Show wind barbs at altitude
- `showIsothermLines`: Show temperature contour lines

### Visualization Components

**Main Meteogram** (`src/app/components/meteogram.tsx`):
- Built with visx (D3-based React visualization library)
- Uses custom hook `useMeteogramScales` for scale calculations
- Renders multiple layers: cloud columns, weather lines, pressure lines, wind barbs
- Interactive: hover to see details, click to freeze tooltip

**Key child components:**
- `CloudColumns`: Renders cloud coverage as colored rectangles by pressure level
- `WeatherLines`: Draws temperature isotherms (when enabled)
- `PressureLines`: Draws isobaric lines (when enabled)
- `WindBarb`: Renders aviation-style wind barbs at specific points
- `MeteogramTooltip`: Shows detailed data on hover/click
- `HoverIndicators`: Crosshair indicators for hovered cell

### Data Types

**Core types** (`src/types/weather.ts`):
- `CloudCell`: Single pressure level data (hPa, MSL altitude, cloud coverage, temp, wind)
- `CloudColumn`: Time-series point containing array of CloudCells + ground temp
- `WeatherModel`: Union type of supported models
- `VisualizationPreferences`: User display preferences

**Important conversions:**
- Geopotential height → MSL height: `geopotentialToMsl()` in `src/utils/weather.ts`
- Meters → Feet: multiply by `FEET_PER_METER` (3.28084)

### Styling

- TailwindCSS for utility classes
- HeroUI (formerly NextUI) component library for dropdowns and UI controls
- Framer Motion for animations
- Custom CSS in `src/app/globals.css`

## Development Notes

### Adding a New Location

Add to the `LOCATIONS` object in `src/config/weather.ts`:
```typescript
LOCATIONS: {
  KXYZ: {
    longitude: -73.5,
    latitude: 40.5,
  },
}
```

### Adding a New Weather Model

1. Add model name to `MODEL_NAMES` array in `src/config/weather.ts`
2. Add model configuration to `MODEL_CONFIGS` object with appropriate ModelConfig parameters
3. Update `WeatherModel` type in `src/types/weather.ts`

### Working with Scales

Scales are computed in `src/hooks/useMeteogramScales.ts`:
- `dateScale`: Maps Date → x-coordinate (scaleUtc)
- `mslScale`: Maps altitude (ft MSL) → y-coordinate (scaleLinear, inverted)
- `cloudCoverageScale`: Maps cloud % → opacity/color intensity

### Performance Considerations

- The meteogram uses React.memo and useMemo extensively to avoid re-renders
- Wind barbs and isotherms are filtered by step intervals to reduce visual clutter
- Data transformation happens server-side to reduce client work
- Next.js generates static params for predefined locations but uses force-dynamic for custom coordinates
