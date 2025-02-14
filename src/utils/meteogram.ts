import { CloudCell, CloudColumn } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";

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

// Helper function to find freezing levels in a cloud column
export const findFreezingLevels = (
  cloudColumn: CloudCell[],
  groundTemp: number | null,
): number[] => {
  const freezingLevels: number[] = [];

  if (!cloudColumn?.length) {
    return freezingLevels;
  }

  const sortedColumn = [...cloudColumn]
    .filter(
      (cell) =>
        cell.temperature != null && cell.mslFt != null && cell.hpa != null,
    )
    .sort((a, b) => b.hpa - a.hpa);

  if (sortedColumn.length === 0) {
    return freezingLevels;
  }

  if (groundTemp != null && groundTemp <= 0) {
    freezingLevels.push(0);
  } else if (
    groundTemp != null &&
    groundTemp > 0 &&
    sortedColumn[0].temperature <= 0
  ) {
    const t1 = groundTemp;
    const t2 = sortedColumn[0].temperature;
    const h1 = 2 * FEET_PER_METER;
    const h2 = sortedColumn[0].mslFt;
    const freezingLevel = h1 + ((0 - t1) * (h2 - h1)) / (t2 - t1);
    freezingLevels.push(freezingLevel);
  }

  for (let i = 0; i < sortedColumn.length - 1; i++) {
    if (
      sortedColumn[i].temperature > 0 &&
      sortedColumn[i + 1].temperature <= 0
    ) {
      const t1 = sortedColumn[i].temperature;
      const t2 = sortedColumn[i + 1].temperature;
      const h1 = sortedColumn[i].mslFt;
      const h2 = sortedColumn[i + 1].mslFt;
      const freezingLevel = h1 + ((0 - t1) * (h2 - h1)) / (t2 - t1);
      freezingLevels.push(freezingLevel);
    }
  }

  return freezingLevels.sort((a, b) => a - b);
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

  const isotherms: { temp: number; points: { x: number; y: number }[] }[] = [];
  let minTemp = Infinity;
  let maxTemp = -Infinity;

  weatherData.forEach((column) => {
    if (!column.cloud?.length) return;
    column.cloud.forEach((cell) => {
      if (cell.temperature == null) return;
      minTemp = Math.min(minTemp, cell.temperature);
      maxTemp = Math.max(maxTemp, cell.temperature);
    });
  });

  if (minTemp === Infinity || maxTemp === -Infinity) {
    return [];
  }

  minTemp = Math.floor(minTemp / tempStep) * tempStep;
  maxTemp = Math.ceil(maxTemp / tempStep) * tempStep;

  for (let temp = minTemp; temp <= maxTemp; temp += tempStep) {
    let activeLines: { points: { x: number; y: number }[] }[] = [];

    weatherData.forEach((column, colIndex) => {
      if (!column.cloud?.length) return;

      const heightsAtTemp: number[] = [];

      for (let i = 0; i < column.cloud.length - 1; i++) {
        const cell1 = column.cloud[i];
        const cell2 = column.cloud[i + 1];

        if (
          cell1.temperature == null ||
          cell2.temperature == null ||
          cell1.geopotentialFt == null ||
          cell2.geopotentialFt == null
        ) {
          continue;
        }

        if (
          (cell1.temperature <= temp && cell2.temperature >= temp) ||
          (cell1.temperature >= temp && cell2.temperature <= temp)
        ) {
          const ratio =
            (temp - cell1.temperature) /
            (cell2.temperature - cell1.temperature);
          const height =
            cell1.geopotentialFt +
            ratio * (cell2.geopotentialFt - cell1.geopotentialFt);
          heightsAtTemp.push(height);
        }
      }

      if (heightsAtTemp.length === 0) {
        const linesToEnd = activeLines.filter((line) => {
          const lastColIndex = line.points[line.points.length - 1].x;
          return colIndex - lastColIndex >= maxStepDistance;
        });

        linesToEnd.forEach((line) => {
          if (line.points.length > 1) {
            isotherms.push({ temp, points: [...line.points] });
          }
        });

        activeLines = activeLines.filter((line) => {
          const lastColIndex = line.points[line.points.length - 1].x;
          return colIndex - lastColIndex < maxStepDistance;
        });
        return;
      }

      heightsAtTemp.sort((a, b) => a - b);
      const groupedHeights = heightsAtTemp.reduce(
        (groups: number[], height) => {
          if (
            groups.length === 0 ||
            height - groups[groups.length - 1] >= heightThreshold
          ) {
            groups.push(height);
          }
          return groups;
        },
        [],
      );

      const newActiveLines: typeof activeLines = [];
      const usedHeights = new Set<number>();

      activeLines.forEach((line) => {
        const lastColIndex = line.points[line.points.length - 1].x;
        if (colIndex - lastColIndex <= maxStepDistance) {
          const lastHeight = line.points[line.points.length - 1].y;
          let bestMatch = groupedHeights[0];
          let minDiff = Math.abs(bestMatch - lastHeight);

          groupedHeights.forEach((height) => {
            if (!usedHeights.has(height)) {
              const diff = Math.abs(height - lastHeight);
              if (diff < minDiff) {
                minDiff = diff;
                bestMatch = height;
              }
            }
          });

          if (minDiff < heightThreshold * 4) {
            line.points.push({ x: colIndex, y: bestMatch });
            usedHeights.add(bestMatch);
            newActiveLines.push(line);
          } else if (line.points.length > 1) {
            isotherms.push({ temp, points: [...line.points] });
          }
        } else if (line.points.length > 1) {
          isotherms.push({ temp, points: [...line.points] });
        }
      });

      groupedHeights.forEach((height) => {
        if (!usedHeights.has(height)) {
          newActiveLines.push({
            points: [{ x: colIndex, y: height }],
          });
        }
      });

      activeLines = newActiveLines;
    });

    activeLines.forEach((line) => {
      if (line.points.length > 1) {
        isotherms.push({ temp, points: [...line.points] });
      }
    });
  }

  return isotherms;
};
