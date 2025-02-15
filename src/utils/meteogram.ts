import { CloudCell, CloudColumn } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import { isoLines } from "marching-squares";

export const formatNumber = (num: number | undefined): number =>
  num ? Number(num.toFixed(4)) : 0;

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

// Helper function to interpolate altitude for a given temperature between two points
const interpolateAltitude = (
  temp: number,
  point1: { temp: number; altitude: number },
  point2: { temp: number; altitude: number },
): number => {
  const ratio = (temp - point1.temp) / (point2.temp - point1.temp);
  return point1.altitude + ratio * (point2.altitude - point1.altitude);
};

// Convert weather data to a high-resolution temperature grid
const createInterpolatedTempGrid = (
  weatherData: CloudColumn[],
  resolution: number = 100,
  includeGround: boolean = false,
): number[][] => {
  if (!weatherData.length || !weatherData[0].cloud.length) return [];

  // Find altitude range
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

  // Add some padding to the altitude range to avoid edge effects
  const altPadding = (maxAlt - minAlt) * 0.1;
  minAlt -= altPadding;
  maxAlt += altPadding;

  // Create a high-resolution grid
  const grid: number[][] = [];
  const altStep = (maxAlt - minAlt) / (resolution - 1);

  // For each altitude level
  for (let i = 0; i < resolution; i++) {
    const altitude = minAlt + i * altStep;
    const row: number[] = [];

    // For each time step
    for (let timeIndex = 0; timeIndex < weatherData.length; timeIndex++) {
      const column = weatherData[timeIndex];
      const sortedCells = [...column.cloud]
        .filter((cell) => cell.mslFt != null && cell.temperature != null)
        .sort((a, b) => a.mslFt - b.mslFt);

      if (sortedCells.length < 2) {
        row.push(NaN);
        continue;
      }

      // Handle points below the lowest measurement
      if (altitude <= sortedCells[0].mslFt) {
        if (includeGround && column.groundTemp != null) {
          // Interpolate between ground temperature and lowest measurement
          const ratio = altitude / sortedCells[0].mslFt;
          row.push(
            column.groundTemp +
              ratio * (sortedCells[0].temperature! - column.groundTemp),
          );
        } else {
          row.push(sortedCells[0].temperature!);
        }
        continue;
      }

      // Handle points above the highest measurement
      if (altitude >= sortedCells[sortedCells.length - 1].mslFt) {
        row.push(sortedCells[sortedCells.length - 1].temperature!);
        continue;
      }

      // Find cells that bracket this altitude
      let found = false;
      for (let j = 0; j < sortedCells.length - 1; j++) {
        const cell1 = sortedCells[j];
        const cell2 = sortedCells[j + 1];

        if (cell1.mslFt <= altitude && cell2.mslFt >= altitude) {
          // Interpolate temperature at this altitude
          const ratio = (altitude - cell1.mslFt) / (cell2.mslFt - cell1.mslFt);
          row.push(
            cell1.temperature! +
              ratio * (cell2.temperature! - cell1.temperature!),
          );
          found = true;
          break;
        }
      }

      if (!found) {
        row.push(NaN);
      }
    }
    grid.push(row);
  }

  // Fill any remaining NaN values using nearest neighbor
  for (let i = 0; i < grid.length; i++) {
    for (let j = 0; j < grid[i].length; j++) {
      if (isNaN(grid[i][j])) {
        // Look for nearest non-NaN value vertically
        let above = i - 1;
        let below = i + 1;
        while (above >= 0 && isNaN(grid[above][j])) above--;
        while (below < grid.length && isNaN(grid[below][j])) below++;

        if (above >= 0 && below < grid.length) {
          // Interpolate between above and below
          const ratio = (i - above) / (below - above);
          grid[i][j] =
            grid[above][j] + ratio * (grid[below][j] - grid[above][j]);
        } else if (above >= 0) {
          grid[i][j] = grid[above][j];
        } else if (below < grid.length) {
          grid[i][j] = grid[below][j];
        }
      }
    }
  }

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

    const validRange = findValidAltitudeRange(weatherData[timeIndex]);
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
