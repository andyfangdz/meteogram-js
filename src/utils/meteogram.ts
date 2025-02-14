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

interface IsothermPoint {
  x: number;
  y: number;
  temp?: number;
}

interface IsothermPath {
  points: IsothermPoint[];
  minHeight: number;
  maxHeight: number;
}

// Helper function to check if paths are similar
const arePathsSimilar = (
  path1: IsothermPath,
  path2: IsothermPath,
  heightThreshold: number,
): boolean => {
  // Check if paths overlap in height range
  const heightOverlap = !(
    path1.maxHeight < path2.minHeight - heightThreshold ||
    path1.minHeight > path2.maxHeight + heightThreshold
  );

  if (!heightOverlap) return false;

  // Check if paths share similar points
  let similarPoints = 0;
  let totalPoints = 0;

  // Compare points at same x coordinates
  const points1ByX = new Map(path1.points.map((p) => [p.x, p.y]));
  const points2ByX = new Map(path2.points.map((p) => [p.x, p.y]));

  points1ByX.forEach((y1, x) => {
    const y2 = points2ByX.get(x);
    if (y2 !== undefined) {
      totalPoints++;
      if (Math.abs(y1 - y2) < heightThreshold) {
        similarPoints++;
      }
    }
  });

  // Paths are similar if they share enough similar points
  return totalPoints > 0 && similarPoints / totalPoints > 0.5;
};

// Helper function to analyze temperature profile in a column
const analyzeTemperatureProfile = (
  levels: CloudCell[],
  targetTemp: number,
  heightThreshold: number,
): {
  validCrossings: { height: number; temp: number }[];
  inversions: { bottom: number; top: number; increasing: boolean }[];
} => {
  if (levels.length < 2) return { validCrossings: [], inversions: [] };

  const sortedLevels = [...levels]
    .filter((cell) => cell.geopotentialFt != null && cell.temperature != null)
    .sort((a, b) => a.geopotentialFt! - b.geopotentialFt!);

  const validCrossings: { height: number; temp: number }[] = [];
  const inversions: { bottom: number; top: number; increasing: boolean }[] = [];

  // First pass: Find all potential crossings
  for (let i = 0; i < sortedLevels.length - 1; i++) {
    const cell1 = sortedLevels[i];
    const cell2 = sortedLevels[i + 1];

    // Check for exact matches
    if (Math.abs(cell1.temperature! - targetTemp) < 0.01) {
      validCrossings.push({
        height: cell1.geopotentialFt!,
        temp: cell1.temperature!,
      });
    }

    // Check for crossings between levels
    if (
      (cell1.temperature! <= targetTemp && cell2.temperature! >= targetTemp) ||
      (cell1.temperature! >= targetTemp && cell2.temperature! <= targetTemp)
    ) {
      const ratio =
        (targetTemp - cell1.temperature!) /
        (cell2.temperature! - cell1.temperature!);
      const height =
        cell1.geopotentialFt! +
        ratio * (cell2.geopotentialFt! - cell1.geopotentialFt!);
      validCrossings.push({ height, temp: targetTemp });
    }
  }

  // Second pass: Identify inversions
  let currentTrend = 0; // -1 decreasing, 0 unknown, 1 increasing
  let trendStart = 0;

  for (let i = 1; i < sortedLevels.length; i++) {
    const prev = sortedLevels[i - 1];
    const curr = sortedLevels[i];
    const lapseRate =
      (curr.temperature! - prev.temperature!) /
      (curr.geopotentialFt! - prev.geopotentialFt!);

    const newTrend = lapseRate > 0 ? 1 : -1;

    if (currentTrend !== 0 && newTrend !== currentTrend) {
      // We found an inversion
      inversions.push({
        bottom: sortedLevels[trendStart].geopotentialFt!,
        top: curr.geopotentialFt!,
        increasing: currentTrend > 0,
      });
      trendStart = i;
    }

    if (currentTrend === 0) {
      currentTrend = newTrend;
      trendStart = i - 1;
    }
  }

  // Third pass: Filter out duplicates and validate crossings
  validCrossings.sort((a, b) => a.height - b.height);
  const filteredCrossings = validCrossings.filter((crossing, i) => {
    // Always keep the first crossing
    if (i === 0) return true;

    const prevCrossing = validCrossings[i - 1];
    const heightDiff = Math.abs(crossing.height - prevCrossing.height);

    // If points are close together, only keep if in an inversion
    if (heightDiff < heightThreshold) {
      const inInversion = inversions.some(
        (inv) => crossing.height >= inv.bottom && crossing.height <= inv.top,
      );
      return inInversion;
    }

    return true;
  });

  return { validCrossings: filteredCrossings, inversions };
};

// Helper function to predict next point in an inversion
const predictNextPoint = (
  path: IsothermPath,
  inversions: { bottom: number; top: number; increasing: boolean }[],
  heightThreshold: number,
): IsothermPoint | null => {
  const lastPoint = path.points[path.points.length - 1];
  if (path.points.length < 2) return null;

  const prevPoint = path.points[path.points.length - 2];
  const verticalRate =
    (lastPoint.y - prevPoint.y) / (lastPoint.x - prevPoint.x);

  // Check if we're in an inversion
  const matchingInversion = inversions.find(
    (inv) =>
      lastPoint.y >= inv.bottom - heightThreshold &&
      lastPoint.y <= inv.top + heightThreshold,
  );

  if (matchingInversion) {
    // Follow the inversion pattern
    const inMiddleOfInversion =
      lastPoint.y > matchingInversion.bottom + heightThreshold &&
      lastPoint.y < matchingInversion.top - heightThreshold;

    if (inMiddleOfInversion) {
      // Continue current trend more aggressively
      return {
        x: lastPoint.x + 1,
        y: lastPoint.y + verticalRate * 2,
      } as IsothermPoint;
    } else {
      // Near the edges, be more conservative
      return {
        x: lastPoint.x + 1,
        y: lastPoint.y + verticalRate * 0.5,
      } as IsothermPoint;
    }
  }

  // Outside inversions, follow normal trend
  return {
    x: lastPoint.x + 1,
    y: lastPoint.y + verticalRate * 0.75,
  } as IsothermPoint;
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

  // Find temperature range
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

  // For each temperature, find all possible isotherm paths
  for (let temp = minTemp; temp <= maxTemp; temp += tempStep) {
    // First pass: Find all points where temperature equals our target
    const allPoints: IsothermPoint[][] = [];

    weatherData.forEach((column, colIndex) => {
      if (!column.cloud?.length) return;

      const columnPoints: IsothermPoint[] = [];
      const sortedLevels = [...column.cloud]
        .filter(
          (cell) => cell.geopotentialFt != null && cell.temperature != null,
        )
        .sort((a, b) => a.geopotentialFt! - b.geopotentialFt!);

      if (sortedLevels.length < 2) return;

      // Check each pair of adjacent levels for crossings
      for (let i = 0; i < sortedLevels.length - 1; i++) {
        const cell1 = sortedLevels[i];
        const cell2 = sortedLevels[i + 1];

        // Check for exact matches at pressure levels
        if (Math.abs(cell1.temperature! - temp) < tempStep / 4) {
          columnPoints.push({
            x: colIndex,
            y: cell1.geopotentialFt!,
            temp: cell1.temperature!,
          });
        }

        // Check for crossings between levels
        if (
          (cell1.temperature! <= temp && cell2.temperature! >= temp) ||
          (cell1.temperature! >= temp && cell2.temperature! <= temp)
        ) {
          const ratio =
            (temp - cell1.temperature!) /
            (cell2.temperature! - cell1.temperature!);
          const height =
            cell1.geopotentialFt! +
            ratio * (cell2.geopotentialFt! - cell1.geopotentialFt!);
          columnPoints.push({
            x: colIndex,
            y: height,
            temp: temp,
          });
        }
      }

      // Check last level for exact match
      const lastCell = sortedLevels[sortedLevels.length - 1];
      if (Math.abs(lastCell.temperature! - temp) < tempStep / 4) {
        columnPoints.push({
          x: colIndex,
          y: lastCell.geopotentialFt!,
          temp: lastCell.temperature!,
        });
      }

      if (columnPoints.length > 0) {
        // Analyze temperature profile and get valid crossings
        const { validCrossings, inversions } = analyzeTemperatureProfile(
          sortedLevels,
          temp,
          heightThreshold,
        );

        // Convert valid crossings to points
        const uniquePoints = validCrossings.map((crossing) => ({
          x: colIndex,
          y: crossing.height,
          temp: crossing.temp,
        }));

        if (uniquePoints.length > 0) {
          allPoints[colIndex] = uniquePoints;
        }
      }
    });

    // Second pass: Connect points into lines
    const activePaths: IsothermPath[] = [];

    // Process each column
    for (let colIndex = 0; colIndex < weatherData.length; colIndex++) {
      const columnPoints = allPoints[colIndex] || [];

      if (columnPoints.length === 0) {
        // End paths that can't be continued
        activePaths.forEach((path) => {
          const lastX = path.points[path.points.length - 1].x;
          if (colIndex - lastX >= maxStepDistance && path.points.length > 1) {
            isotherms.push({ temp, points: [...path.points] });
          }
        });
        continue;
      }

      if (activePaths.length === 0) {
        // Start new paths
        columnPoints.forEach((point) => {
          activePaths.push({
            points: [{ x: point.x, y: point.y }],
            minHeight: point.y,
            maxHeight: point.y,
          });
        });
        continue;
      }

      // Try to continue each active path
      const newPaths: IsothermPath[] = [];
      const usedPoints = new Set<number>();

      // First, try to continue existing paths
      activePaths.forEach((path) => {
        const lastPoint = path.points[path.points.length - 1];
        const lastX = lastPoint.x;

        if (colIndex - lastX > maxStepDistance) {
          // Try to predict where the path should go
          const prediction = predictNextPoint(
            path,
            analyzeTemperatureProfile(
              weatherData[lastX].cloud!,
              temp,
              heightThreshold,
            ).inversions,
            heightThreshold,
          );

          let foundContinuation = false;
          if (prediction) {
            // Look for points near the predicted location
            const futurePoints = allPoints[colIndex] || [];
            const bestMatch = futurePoints.find(
              (p) => Math.abs(p.y - prediction.y) < heightThreshold * 1.5,
            );

            if (bestMatch) {
              // Bridge the gap with interpolated points
              for (let bridgeX = lastX + 1; bridgeX < colIndex; bridgeX++) {
                const ratio = (bridgeX - lastX) / (colIndex - lastX);
                const bridgeY =
                  lastPoint.y + (bestMatch.y - lastPoint.y) * ratio;
                path.points.push({ x: bridgeX, y: bridgeY });
              }
              path.points.push({ x: bestMatch.x, y: bestMatch.y });
              foundContinuation = true;
            }
          }

          if (!foundContinuation && path.points.length > 1) {
            isotherms.push({ temp, points: [...path.points] });
          }
          return;
        }

        // Find closest point within threshold, preferring points that follow the trend
        let minCost = heightThreshold * 2;
        type MatchType = {
          point: IsothermPoint;
          index: number;
        };
        let bestMatch: MatchType | null = null;

        columnPoints.forEach((point: IsothermPoint, i: number) => {
          if (usedPoints.has(i)) return;

          const verticalDist = Math.abs(point.y - lastPoint.y);
          let cost = verticalDist;

          // Add cost if point deviates from expected trend
          if (path.points.length > 1) {
            const prevPoint = path.points[path.points.length - 2];
            const expectedY =
              lastPoint.y +
              (lastPoint.y - prevPoint.y) / (lastPoint.x - prevPoint.x);
            cost += Math.abs(point.y - expectedY) * 0.5;
          }

          if (cost < minCost) {
            minCost = cost;
            bestMatch = { point, index: i };
          }
        });

        if (bestMatch) {
          bestMatch = bestMatch as MatchType;
          usedPoints.add(bestMatch.index);
          // If there's a gap, add interpolated points
          if (colIndex - lastX > 1) {
            for (let bridgeX = lastX + 1; bridgeX < colIndex; bridgeX++) {
              const ratio = (bridgeX - lastX) / (colIndex - lastX);
              const bridgeY =
                lastPoint.y + (bestMatch.point.y - lastPoint.y) * ratio;
              path.points.push({ x: bridgeX, y: bridgeY });
            }
          }
          const newPath = {
            points: [
              ...path.points,
              { x: bestMatch.point.x, y: bestMatch.point.y },
            ],
            minHeight: Math.min(path.minHeight, bestMatch.point.y),
            maxHeight: Math.max(path.maxHeight, bestMatch.point.y),
          };

          if (
            !newPaths.some((p) => arePathsSimilar(p, newPath, heightThreshold))
          ) {
            newPaths.push(newPath);
          }
        } else if (path.points.length > 1) {
          isotherms.push({ temp, points: [...path.points] });
        }
      });

      // Start new paths from unused points, but only if they're not similar to existing paths
      columnPoints.forEach((point, i) => {
        if (!usedPoints.has(i)) {
          const newPath = {
            points: [{ x: point.x, y: point.y }],
            minHeight: point.y,
            maxHeight: point.y,
          };

          if (
            !newPaths.some((p) => arePathsSimilar(p, newPath, heightThreshold))
          ) {
            newPaths.push(newPath);
          }
        }
      });

      activePaths.splice(0, activePaths.length, ...newPaths);
    }

    // Filter similar paths before adding to isotherms
    const uniquePaths = activePaths.reduce((acc: IsothermPath[], path) => {
      if (
        path.points.length > 1 &&
        !acc.some((p) => arePathsSimilar(p, path, heightThreshold))
      ) {
        acc.push(path);
      }
      return acc;
    }, []);

    // Add remaining unique paths
    uniquePaths.forEach((path) => {
      isotherms.push({ temp, points: [...path.points] });
    });
  }

  return isotherms;
};
