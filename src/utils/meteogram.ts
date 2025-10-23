import { CloudCell, CloudColumn } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import { isoLines } from "marching-squares";

// Faster numeric rounding to 4 decimals without string conversion
export const formatNumber = (num: number | undefined): number =>
  Number.isFinite(num) ? Math.round((num as number) * 1e4) / 1e4 : 0;

export const hPaToInHg = (hpa: number | undefined): string =>
  hpa ? (hpa * 0.02953).toFixed(2) : "N/A";

export const kmhToKnots = (kmh: number | undefined): string =>
  kmh ? (kmh * 0.539957).toFixed(0) : "N/A";

export const getTemperatureColor = (temp: number): string => {
  if (temp <= -20) return "#800080"; // Very cold (purple)
  if (temp <= 0) return "#0000FF"; // Freezing (blue)
  if (temp >= 30) return "#FF0000"; // Very hot (red)

  // For temperatures between 0 and 30, interpolate between blue and red
  if (temp > 0 && temp < 30) {
    const ratio = temp / 30;
    const r = Math.round(ratio * 255);
    const b = Math.round((1 - ratio) * 255);
    return `rgb(${r},0,${b})`;
  }

  // For temperatures between -20 and 0, interpolate between purple and blue
  const ratio = (temp + 20) / 20;
  const r = Math.round((1 - ratio) * 128);
  const b = 255;
  return `rgb(${r},0,${b})`;
};

export const getWindSpeedColor = (speedKnots: number): string => {
  // Color scheme for wind speed in knots - bold, saturated colors for visibility
  if (speedKnots <= 10) return "#00AA00"; // Dark green (was light green)
  if (speedKnots <= 20) return "#FFD700"; // Gold
  if (speedKnots <= 30) return "#FF8C00"; // Dark orange (was light orange)
  if (speedKnots <= 40) return "#FF4500"; // Orange red (was tomato)
  return "#CC0000"; // Dark red (was red)
};

// Helper function to interpolate altitude for a given temperature between two points
const interpolateAltitude = (
  temp: number,
  point1: { temp: number; altitude: number },
  point2: { temp: number; altitude: number },
): number => {
  const ratio = (temp - point1.temp) / (point2.temp - point1.temp);
  return point1.altitude + ratio * (point2.altitude - point1.altitude);
};

// Generic function to create a high-resolution interpolated grid for any cell property
const createInterpolatedGrid = (
  weatherData: CloudColumn[],
  resolution: number = 100,
  options: {
    valueExtractor: (cell: CloudCell) => number | undefined;
    groundValueExtractor?: (column: CloudColumn) => number | undefined;
  },
): number[][] => {
  if (!weatherData.length || !weatherData[0].cloud.length) return [];

  // Precompute per-timestep sorted levels, values, and ground values
  const perColumn = weatherData.map((column) => {
    const filtered = column.cloud
      .filter((cell) => cell.mslFt != null && options.valueExtractor(cell) != null)
      .sort((a, b) => a.mslFt - b.mslFt);

    const levels = filtered.map((c) => c.mslFt!);
    const values = filtered.map((c) => options.valueExtractor(c)!);
    const groundValue = options.groundValueExtractor?.(column);
    return { levels, values, groundValue };
  });

  // Find altitude range from precomputed levels
  let minAlt = Infinity;
  let maxAlt = -Infinity;
  for (const { levels } of perColumn) {
    if (levels.length) {
      minAlt = Math.min(minAlt, levels[0]);
      maxAlt = Math.max(maxAlt, levels[levels.length - 1]);
    }
  }
  if (minAlt === Infinity || maxAlt === -Infinity) return [];

  // Add some padding to the altitude range to avoid edge effects
  const altPadding = (maxAlt - minAlt) * 0.1;
  minAlt -= altPadding;
  maxAlt += altPadding;

  // Create a high-resolution grid
  const grid: number[][] = new Array(resolution);
  const altStep = (maxAlt - minAlt) / (resolution - 1);

  // For each altitude level
  for (let i = 0; i < resolution; i++) {
    const altitude = minAlt + i * altStep;
    const row: number[] = new Array(perColumn.length);

    // For each time step
    for (let timeIndex = 0; timeIndex < perColumn.length; timeIndex++) {
      const { levels, values, groundValue } = perColumn[timeIndex];
      if (levels.length < 2) {
        row[timeIndex] = NaN;
        continue;
      }

      // Below lowest measurement
      if (altitude <= levels[0]) {
        if (groundValue != null) {
          const ratio = altitude / levels[0];
          row[timeIndex] = groundValue + ratio * (values[0] - groundValue);
        } else {
          row[timeIndex] = values[0];
        }
        continue;
      }

      // Above highest measurement
      const lastIdx = levels.length - 1;
      if (altitude >= levels[lastIdx]) {
        row[timeIndex] = values[lastIdx];
        continue;
      }

      // Binary search for bracketing indices
      let lo = 0;
      let hi = lastIdx;
      while (lo + 1 < hi) {
        const mid = (lo + hi) >> 1;
        if (levels[mid] === altitude) {
          lo = mid;
          hi = mid + 1;
          break;
        }
        if (levels[mid] < altitude) lo = mid; else hi = mid;
      }
      const lowerIdx = lo;
      const upperIdx = hi;
      const lowerAlt = levels[lowerIdx];
      const upperAlt = levels[upperIdx];
      const ratio = (altitude - lowerAlt) / (upperAlt - lowerAlt);
      row[timeIndex] = values[lowerIdx] + ratio * (values[upperIdx] - values[lowerIdx]);
    }
    grid[i] = row;
  }

  // Fill any remaining NaN values using nearest neighbor (vertical)
  for (let j = 0; j < perColumn.length; j++) {
    // Precompute nearest non-NaN above
    let lastValid = NaN;
    for (let i = 0; i < resolution; i++) {
      if (!Number.isNaN(grid[i][j])) {
        lastValid = grid[i][j];
      } else if (!Number.isNaN(lastValid)) {
        grid[i][j] = lastValid;
      }
    }
    // Precompute nearest non-NaN below and interpolate if needed
    lastValid = NaN;
    for (let i = resolution - 1; i >= 0; i--) {
      if (!Number.isNaN(grid[i][j])) {
        lastValid = grid[i][j];
      } else if (!Number.isNaN(lastValid)) {
        // If there was also an above value, average them; else use below
        // Find above value by scanning up until a non-NaN or start
        let k = i - 1;
        while (k >= 0 && Number.isNaN(grid[k][j])) k--;
        if (k >= 0) {
          grid[i][j] = (grid[k][j] + lastValid) / 2;
        } else {
          grid[i][j] = lastValid;
        }
      }
    }
  }

  return grid;
};

// Convert weather data to a high-resolution temperature grid
// Caches to avoid recomputing grids for the same weatherData
const tempGridCache: WeakMap<object, Map<string, number[][]>> = new WeakMap();
const windGridCache: WeakMap<object, Map<string, number[][]>> = new WeakMap();

const createInterpolatedTempGrid = (
  weatherData: CloudColumn[],
  resolution: number = 100,
  includeGround: boolean = false,
): number[][] => {
  const key = `${resolution}|${includeGround ? 1 : 0}`;
  let byOptions = tempGridCache.get(weatherData as unknown as object);
  if (!byOptions) {
    byOptions = new Map();
    tempGridCache.set(weatherData as unknown as object, byOptions);
  }
  const cached = byOptions.get(key);
  if (cached) return cached;

  const grid = createInterpolatedGrid(weatherData, resolution, {
    valueExtractor: (cell) => cell.temperature,
    groundValueExtractor: includeGround
      ? (column) => column.groundTemp
      : undefined,
  });

  byOptions.set(key, grid);
  return grid;
};

// Convert weather data to a high-resolution wind speed grid
const createInterpolatedWindSpeedGrid = (
  weatherData: CloudColumn[],
  resolution: number = 100,
): number[][] => {
  const key = `${resolution}`;
  let byOptions = windGridCache.get(weatherData as unknown as object);
  if (!byOptions) {
    byOptions = new Map();
    windGridCache.set(weatherData as unknown as object, byOptions);
  }
  const cached = byOptions.get(key);
  if (cached) return cached;

  const grid = createInterpolatedGrid(weatherData, resolution, {
    valueExtractor: (cell) => cell.windSpeed,
  });
  byOptions.set(key, grid);
  return grid;
};

// Helper function to convert grid coordinates back to weather data coordinates
const gridToWeatherCoords = (
  x: number,
  y: number,
  weatherData: CloudColumn[],
  minAlt: number,
  maxAlt: number,
  resolution: number,
): { x: number; y: number } => {
  const timeIndex = Math.min(
    Math.max(Math.round(x), 0),
    weatherData.length - 1,
  );
  const altitude = minAlt + (y / (resolution - 1)) * (maxAlt - minAlt);
  return {
    x: timeIndex,
    y: altitude,
  };
};

// Helper function to find the valid altitude range at a given time index
const findValidAltitudeRange = (
  column: CloudColumn | null,
): { min: number; max: number } | null => {
  if (!column) return null;
  const validCells = column.cloud.filter(
    (cell) => cell.mslFt != null && cell.temperature != null,
  );
  if (validCells.length === 0) return null;

  return {
    min: Math.min(...validCells.map((cell) => cell.mslFt!)),
    max: Math.max(...validCells.map((cell) => cell.mslFt!)),
  };
};

// Helper function to clip a line to valid altitude ranges
const clipLineToValidRanges = (
  points: { x: number; y: number }[],
  weatherData: CloudColumn[],
  clipToMinAltitude: boolean = true,
  groundAltitude?: number,
): { x: number; y: number }[] => {
  if (points.length < 2) return points;

  const clippedPoints: { x: number; y: number }[] = [];
  let currentSegment: { x: number; y: number }[] = [];
  const validRangeCache = new Map<number, { min: number; max: number } | null>();

  for (let i = 0; i < points.length; i++) {
    const point = points[i];
    const timeIndex = Math.round(point.x);

    if (timeIndex < 0 || timeIndex >= weatherData.length) {
      if (currentSegment.length > 1) {
        clippedPoints.push(...currentSegment);
        currentSegment = [];
      }
      continue;
    }

    let validRange = validRangeCache.get(timeIndex);
    if (validRange === undefined) {
      validRange = findValidAltitudeRange(weatherData[timeIndex]);
      validRangeCache.set(timeIndex, validRange);
    }
    if (!validRange) {
      if (currentSegment.length > 1) {
        clippedPoints.push(...currentSegment);
        currentSegment = [];
      }
      continue;
    }

    // Get the minimum altitude to clip to
    const minAltitude = clipToMinAltitude
      ? validRange.min
      : (groundAltitude ?? validRange.min);

    // Clip point to valid range
    const clippedPoint = {
      x: point.x,
      y: Math.min(Math.max(point.y, minAltitude), validRange.max),
    };

    // If this is the first point or if it connects to the previous segment
    if (
      currentSegment.length === 0 ||
      Math.abs(
        timeIndex - Math.round(currentSegment[currentSegment.length - 1].x),
      ) <= 1
    ) {
      currentSegment.push(clippedPoint);
    } else {
      // Start a new segment if there's a gap
      if (currentSegment.length > 1) {
        clippedPoints.push(...currentSegment);
      }
      currentSegment = [clippedPoint];
    }
  }

  // Add the last segment if it exists
  if (currentSegment.length > 1) {
    clippedPoints.push(...currentSegment);
  }

  return clippedPoints;
};

// Helper function to find isotherm points
export const findIsothermPoints = (
  weatherData: CloudColumn[],
  tempStep: number = 5,
  heightThreshold: number = 1000,
  maxStepDistance: number = 1,
) => {
  if (!weatherData?.length) {
    return [];
  }

  // Find temperature range
  let minTemp = Infinity;
  let maxTemp = -Infinity;

  weatherData.forEach((column) => {
    column.cloud.forEach((cell) => {
      if (cell.temperature != null) {
        minTemp = Math.min(minTemp, cell.temperature);
        maxTemp = Math.max(maxTemp, cell.temperature);
      }
    });
  });

  if (minTemp === Infinity || maxTemp === -Infinity) {
    return [];
  }

  // Round to nearest tempStep
  minTemp = Math.floor(minTemp / tempStep) * tempStep;
  maxTemp = Math.ceil(maxTemp / tempStep) * tempStep;

  // Generate thresholds
  const thresholds = [];
  for (let temp = minTemp; temp <= maxTemp; temp += tempStep) {
    thresholds.push(temp);
  }

  // Create high-resolution temperature grid
  const resolution = 100; // Increase for more precision
  const tempGrid = createInterpolatedTempGrid(weatherData, resolution);

  if (tempGrid.length === 0) return [];

  // Find altitude range for coordinate conversion
  let minAlt = Infinity;
  let maxAlt = -Infinity;
  weatherData.forEach((column) => {
    column.cloud.forEach((cell) => {
      if (cell.mslFt != null) {
        minAlt = Math.min(minAlt, cell.mslFt);
        maxAlt = Math.max(maxAlt, cell.mslFt);
      }
    });
  });

  if (minAlt === Infinity || maxAlt === -Infinity) return [];

  // Add padding to altitude range
  const altPadding = (maxAlt - minAlt) * 0.1;
  minAlt -= altPadding;
  maxAlt += altPadding;

  try {
    // Use marching-squares to find isotherms
    const lines = isoLines(tempGrid, thresholds, { noFrame: true });

    if (!lines) return [];

    // Convert the lines to our format, keeping separate paths for each temperature
    const result: { temp: number; points: { x: number; y: number }[] }[] = [];

    thresholds.forEach((temp, i) => {
      const tempLines = lines[i] || [];
      tempLines
        .filter((line) => line.length > 2)
        .forEach((line) => {
          // Convert each line to weather coordinates
          const points = line.map(([x, y]) =>
            gridToWeatherCoords(x, y, weatherData, minAlt, maxAlt, resolution),
          );

          // Clip the line to valid ranges and filter out short segments
          const clippedPoints = clipLineToValidRanges(points, weatherData);
          if (clippedPoints.length > 2) {
            result.push({ temp, points: clippedPoints });
          }
        });
    });

    return result;
  } catch (error) {
    console.error("Error computing isotherms:", error);
    return [];
  }
};

// Helper function to find freezing levels in a cloud column
export const findFreezingLevels = (
  weatherData: CloudColumn[],
): { points: { x: number; y: number }[] }[] => {
  if (!weatherData?.length) return [];

  const resolution = 100; // Increase for more precision
  const tempGrid = createInterpolatedTempGrid(weatherData, resolution, false); // Include ground temperature

  if (tempGrid.length === 0) return [];

  // Find altitude range for coordinate conversion
  let minAlt = Infinity;
  let maxAlt = -Infinity;
  weatherData.forEach((column) => {
    column.cloud.forEach((cell) => {
      if (cell.mslFt != null) {
        minAlt = Math.min(minAlt, cell.mslFt);
        maxAlt = Math.max(maxAlt, cell.mslFt);
      }
    });
  });

  if (minAlt === Infinity || maxAlt === -Infinity) return [];

  // Add padding to altitude range
  const altPadding = (maxAlt - minAlt) * 0.1;
  minAlt -= altPadding;
  maxAlt += altPadding;

  try {
    // Use marching-squares to find the freezing level (0°C isoline)
    const lines = isoLines(tempGrid, [0], { noFrame: true });

    if (!lines?.[0]) return [];

    // Convert the lines to our format
    return lines[0]
      .filter((line) => line.length > 2)
      .map((line) => {
        const points = line.map(([x, y]) =>
          gridToWeatherCoords(x, y, weatherData, minAlt, maxAlt, resolution),
        );

        // Clip the line to valid ranges, using ground altitude as the minimum
        const clippedPoints = clipLineToValidRanges(
          points,
          weatherData,
          false,
          0,
        );
        return { points: clippedPoints };
      })
      .filter((line) => line.points.length > 2);
  } catch (error) {
    console.error("Error computing freezing levels:", error);
    return [];
  }
};

// Helper function to find isotach points (constant wind speed lines)
export const findIsotachPoints = (
  weatherData: CloudColumn[],
  speedStepKnots: number = 10,
  heightThreshold: number = 1000,
  maxStepDistance: number = 1,
) => {
  if (!weatherData?.length) {
    return [];
  }

  // Find wind speed range (convert from km/h to knots)
  let minSpeed = Infinity;
  let maxSpeed = -Infinity;

  weatherData.forEach((column) => {
    column.cloud.forEach((cell) => {
      if (cell.windSpeed != null) {
        const speedKnots = cell.windSpeed * 0.539957; // km/h to knots
        minSpeed = Math.min(minSpeed, speedKnots);
        maxSpeed = Math.max(maxSpeed, speedKnots);
      }
    });
  });

  if (minSpeed === Infinity || maxSpeed === -Infinity) {
    return [];
  }

  // Round to nearest speedStepKnots
  minSpeed = Math.floor(minSpeed / speedStepKnots) * speedStepKnots;
  maxSpeed = Math.ceil(maxSpeed / speedStepKnots) * speedStepKnots;

  // Generate thresholds in knots
  const thresholdsKnots = [];
  for (let speed = minSpeed; speed <= maxSpeed; speed += speedStepKnots) {
    if (speed > 0) {
      // Skip 0 knots
      thresholdsKnots.push(speed);
    }
  }

  // Create high-resolution wind speed grid (in km/h)
  const resolution = 100;
  const windSpeedGrid = createInterpolatedWindSpeedGrid(
    weatherData,
    resolution,
  );

  if (windSpeedGrid.length === 0) return [];

  // Convert grid from km/h to knots for marching squares
  const windSpeedGridKnots = windSpeedGrid.map((row) =>
    row.map((speed) => speed * 0.539957),
  );

  // Find altitude range for coordinate conversion
  let minAlt = Infinity;
  let maxAlt = -Infinity;
  weatherData.forEach((column) => {
    column.cloud.forEach((cell) => {
      if (cell.mslFt != null) {
        minAlt = Math.min(minAlt, cell.mslFt);
        maxAlt = Math.max(maxAlt, cell.mslFt);
      }
    });
  });

  if (minAlt === Infinity || maxAlt === -Infinity) return [];

  // Add padding to altitude range
  const altPadding = (maxAlt - minAlt) * 0.1;
  minAlt -= altPadding;
  maxAlt += altPadding;

  try {
    // Use marching-squares to find isotachs
    const lines = isoLines(windSpeedGridKnots, thresholdsKnots, {
      noFrame: true,
    });

    if (!lines) return [];

    // Convert the lines to our format, keeping separate paths for each wind speed
    const result: {
      speedKnots: number;
      points: { x: number; y: number }[];
    }[] = [];

    thresholdsKnots.forEach((speedKnots, i) => {
      const speedLines = lines[i] || [];
      speedLines
        .filter((line) => line.length > 2)
        .forEach((line) => {
          // Convert each line to weather coordinates
          const points = line.map(([x, y]) =>
            gridToWeatherCoords(x, y, weatherData, minAlt, maxAlt, resolution),
          );

          // Clip the line to valid ranges and filter out short segments
          const clippedPoints = clipLineToValidRanges(points, weatherData);
          if (clippedPoints.length > 2) {
            result.push({ speedKnots, points: clippedPoints });
          }
        });
    });

    return result;
  } catch (error) {
    console.error("Error computing isotachs:", error);
    return [];
  }
};
