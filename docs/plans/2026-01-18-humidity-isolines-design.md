# Humidity Isolines Visualization Design

## Overview

Add dew point depression isolines to the meteogram visualization. Dew point depression (temperature minus dew point, also called "spread") indicates how close air is to saturation. This is particularly useful for aviation weather analysis.

## Specification

### Metric
- **Dew Point Depression** = Temperature - Dew Point (in °C)
- Smaller values indicate air closer to saturation (higher moisture)
- Values with dew point depression near 0°C indicate conditions favorable for fog/cloud formation

### Isoline Thresholds
Fixed aviation-relevant values:
- **0°C** - Saturated (fog/cloud conditions)
- **1°C** - Nearly saturated
- **3°C** - Near saturation, fog/low cloud threshold
- **5°C** - Moderate moisture
- **10°C** - Dry air

### Visual Design
- **Colors:** Moisture gradient
  - 0°C: Magenta (`#FF00FF`) - saturated
  - 1°C: Cyan (`#00FFFF`) - nearly saturated
  - 3°C: Dark Cyan (`#00CED1`) - moist
  - 5°C: Yellow (`#FFD700`) - moderate
  - 10°C: Orange (`#FF8C00`) - dry
- **Line style:** Dotted with spaced dots (`strokeDasharray="1,6"`, `strokeLinecap="round"`) to differentiate from isotherms
- **Labels:** Inline labels with delta symbol showing "Δ0°", "Δ1°", "Δ3°", "Δ5°", "Δ10°"

### User Control
- New toggle: `showDewPointDepressionLines` in visualization preferences
- Default: enabled

## Implementation

### Data Layer

**`src/types/weather.ts`**
```typescript
interface CloudCell {
  // ... existing fields
  dewPoint: number;  // Dew point in Celsius
}

interface VisualizationPreferences {
  // ... existing fields
  showDewPointDepressionLines: boolean;
}
```

**`src/app/actions/weather.ts`**
- Add `dew_point` to pressure level variables in Open-Meteo API request

**`src/utils/weather.ts`**
- Extract `dew_point` from API response
- Populate `CloudCell.dewPoint` during transformation

### Calculation Layer

**`src/utils/meteogram.ts`**

New function following `findIsothermPoints()` pattern:

```typescript
function createInterpolatedDewPointDepressionGrid(
  weatherData: CloudColumn[],
  altitudes: number[],
  gridWidth: number
): number[][]

function findDewPointDepressionPoints(
  weatherData: CloudColumn[],
  thresholds: number[]  // [0, 1, 3, 5, 10]
): DewPointDepressionLine[]

interface DewPointDepressionLine {
  spread: number;      // 0, 1, 3, 5, or 10
  points: Point[];     // {x: timeIndex, y: altitude}
}
```

### Rendering Layer

**`src/app/components/weather-lines.tsx`**
- Add color constants for dew point depression lines
- Render isolines using existing SVG path pattern
- Add inline labels

**`src/config/preferences.ts`**
- Add default: `showDewPointDepressionLines: true`

**`src/app/components/meteogram.tsx`**
- Compute dew point depression points when preference enabled
- Pass to WeatherLines component

## Files Changed

| File | Change |
|------|--------|
| `src/types/weather.ts` | Add `dewPoint` field and preference |
| `src/app/actions/weather.ts` | Add API variable |
| `src/utils/weather.ts` | Extract dew point from response |
| `src/utils/meteogram.ts` | Add isoline calculation functions |
| `src/app/components/weather-lines.tsx` | Render isolines |
| `src/config/preferences.ts` | Add default preference |
| `src/app/components/meteogram.tsx` | Wire up computation and rendering |
