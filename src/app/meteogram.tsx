"use client";

import React, { useState, useMemo, useCallback } from "react";
import { Group } from "@visx/group";
import { AxisLeft } from "@visx/axis";
import { scaleLinear, scaleTime } from "@visx/scale";
import { CloudColumn, CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import LoadingSkeleton from "./loading-skeleton";
import WindBarb from "./components/wind-barb";
import TimeAxis from "./components/time-axis";
import { WIND_BARB_LEVELS, MODEL_CONFIGS } from "../config/weather";
import { WeatherModel } from "../types/weather";

const hPaToInHg = (hpa: number | undefined) =>
  hpa ? (hpa * 0.02953).toFixed(2) : "N/A";
// Convert km/h to knots
const kmhToKnots = (kmh: number | undefined) =>
  kmh ? (kmh * 0.539957).toFixed(0) : "N/A";

// Add this helper function at the top with other constants
const formatNumber = (num: number | undefined) =>
  num ? Number(num.toFixed(4)) : 0;

// Helper function to find freezing levels in a cloud column
const findFreezingLevels = (
  cloudColumn: CloudCell[],
  groundTemp: number | null,
): number[] => {
  const freezingLevels: number[] = [];

  // Return empty array if cloudColumn is empty or null
  if (!cloudColumn?.length) {
    return freezingLevels;
  }

  // Sort the cloud column by pressure (height) from ground up
  const sortedColumn = [...cloudColumn]
    .filter(
      (cell) =>
        cell.temperature != null && cell.mslFt != null && cell.hpa != null,
    )
    .sort((a, b) => b.hpa - a.hpa);

  if (sortedColumn.length === 0) {
    return freezingLevels;
  }

  // Check if ground level is already below freezing
  if (groundTemp != null && groundTemp <= 0) {
    freezingLevels.push(0); // Start from ground level
  }
  // Check if there's a crossing point between ground and first pressure level
  // Only if we're going from above freezing to below freezing
  else if (
    groundTemp != null &&
    groundTemp > 0 &&
    sortedColumn[0].temperature <= 0
  ) {
    // Linear interpolation between ground (2m) and first pressure level
    const t1 = groundTemp;
    const t2 = sortedColumn[0].temperature;
    const h1 = 2 * FEET_PER_METER; // 2 meters above ground
    const h2 = sortedColumn[0].mslFt;
    const freezingLevel = h1 + ((0 - t1) * (h2 - h1)) / (t2 - t1);
    freezingLevels.push(freezingLevel);
  }

  // Find crossing points between pressure levels
  // Only consider transitions from above freezing to below freezing
  for (let i = 0; i < sortedColumn.length - 1; i++) {
    if (
      sortedColumn[i].temperature > 0 &&
      sortedColumn[i + 1].temperature <= 0
    ) {
      // Linear interpolation to find exact freezing level
      const t1 = sortedColumn[i].temperature;
      const t2 = sortedColumn[i + 1].temperature;
      const h1 = sortedColumn[i].mslFt;
      const h2 = sortedColumn[i + 1].mslFt;
      const freezingLevel = h1 + ((0 - t1) * (h2 - h1)) / (t2 - t1);
      freezingLevels.push(freezingLevel);
    }
  }

  // Sort freezing levels by height
  return freezingLevels.sort((a, b) => a - b);
};

// Helper function to match freezing levels between columns
const matchFreezingLevels = (
  levels1: number[],
  levels2: number[],
): [number, number][] => {
  if (levels1.length === 0 || levels2.length === 0) return [];

  const matches: [number, number][] = [];
  const used2 = new Set<number>();

  // First, always match the lowest freezing levels if they exist
  if (levels1.length > 0 && levels2.length > 0) {
    const lowestLevel1 = Math.min(...levels1);
    const lowestLevel2 = Math.min(...levels2);
    matches.push([lowestLevel1, lowestLevel2]);
    used2.add(levels2.indexOf(lowestLevel2));

    // Remove the lowest levels from consideration for other matches
    levels1 = levels1.filter((l) => l !== lowestLevel1);
    levels2 = levels2.filter((l) => l !== lowestLevel2);
  }

  // Then match remaining levels based on proximity
  for (let i = 0; i < levels1.length; i++) {
    let bestMatch = -1;
    let minDiff = Infinity;

    // Find the closest unused level in the second column
    for (let j = 0; j < levels2.length; j++) {
      if (!used2.has(j)) {
        const diff = Math.abs(levels1[i] - levels2[j]);
        if (diff < minDiff) {
          minDiff = diff;
          bestMatch = j;
        }
      }
    }

    // If we found a match within a reasonable height difference (e.g., 5000 ft)
    if (bestMatch !== -1 && minDiff < 5000) {
      matches.push([levels1[i], levels2[bestMatch]]);
      used2.add(bestMatch);
    }
  }

  // Sort matches by height to maintain visual order
  return matches.sort((a, b) => a[0] - b[0]);
};

// Memoized WindBarb component
const MemoizedWindBarb = React.memo(WindBarb);

export type MeteogramProps = {
  width: number;
  height: number;
  margin?: { top: number; right: number; bottom: number; left: number };
  useLocalTime?: boolean;
  weatherData: CloudColumn[];
  highlightCeilingCoverage?: boolean;
  clampCloudCoverageAt50Pct?: boolean;
  isLoading?: boolean;
  showPressureLines?: boolean;
  showWindBarbs?: boolean;
  showIsothermLines?: boolean;
  model: WeatherModel;
};

const black = "#000000";
const background = "#87CEEB";
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

// Helper function to get color for temperature
const getTemperatureColor = (temp: number): string => {
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

// Helper function to find freezing level points
const findFreezingPoints = (weatherData: CloudColumn[]) => {
  const freezingLines: { points: { x: number; y: number }[] }[] = [];
  // const heightThreshold = 3000; // Feet - increased to be more lenient

  // Process each column in sequence
  weatherData.forEach((column, colIndex) => {
    const levels = findFreezingLevels(column.cloud, column.groundTemp);

    if (levels.length === 0) {
      return;
    }

    // Sort levels by height
    levels.sort((a, b) => a - b);

    // Try to continue existing lines or start new ones
    levels.forEach((level) => {
      // Try to find a line to continue
      let foundLine = false;

      for (const line of freezingLines) {
        if (line.points.length === 0) continue;

        const lastPoint = line.points[line.points.length - 1];
        // Only try to connect if this is the next column
        if (lastPoint.x === colIndex - 1) {
          const heightDiff = Math.abs(lastPoint.y - level);
          if (heightDiff < 3000) {
            line.points.push({ x: colIndex, y: level });
            foundLine = true;
            break;
          }
        }
      }

      // If we couldn't continue any existing line, start a new one
      if (!foundLine) {
        freezingLines.push({
          points: [{ x: colIndex, y: level }],
        });
      }
    });
  });

  // Filter out very short lines (less than 3 points)
  return freezingLines
    .filter((line) => line.points.length > 2)
    .sort((a, b) => a.points[0].y - b.points[0].y);
};

// Helper function to find points with similar temperatures
const findIsothermPoints = (
  weatherData: CloudColumn[],
  tempStep: number = 5,
  heightThreshold: number = 1000,
  model: WeatherModel,
) => {
  if (!weatherData?.length) {
    return [];
  }

  const modelConfig = MODEL_CONFIGS[model];
  if (!modelConfig) {
    return [];
  }

  const maxStepDistance = modelConfig.maxIsothermStepDistance || 1;
  const isotherms: { temp: number; points: { x: number; y: number }[] }[] = [];

  // Find min and max temperatures across all data
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

  // If no valid temperatures found, return empty array
  if (minTemp === Infinity || maxTemp === -Infinity) {
    return [];
  }

  // Round to nearest tempStep
  minTemp = Math.floor(minTemp / tempStep) * tempStep;
  maxTemp = Math.ceil(maxTemp / tempStep) * tempStep;

  // For each temperature step
  for (let temp = minTemp; temp <= maxTemp; temp += tempStep) {
    let activeLines: { points: { x: number; y: number }[] }[] = [];

    // Process each column in sequence
    weatherData.forEach((column, colIndex) => {
      if (!column.cloud?.length) return;

      const heightsAtTemp: number[] = [];

      // Find points where temperature crosses our target temperature
      for (let i = 0; i < column.cloud.length - 1; i++) {
        const cell1 = column.cloud[i];
        const cell2 = column.cloud[i + 1];

        // Skip if any required values are null/undefined
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
        // Check if we should end lines based on maxStepDistance
        const linesToEnd: typeof activeLines = [];
        const linesToKeep: typeof activeLines = [];

        activeLines.forEach((line) => {
          const lastColIndex = line.points[line.points.length - 1].x;
          if (colIndex - lastColIndex >= maxStepDistance) {
            if (line.points.length > 1) {
              linesToEnd.push(line);
            }
          } else {
            linesToKeep.push(line);
          }
        });

        // End lines that exceed maxStepDistance
        linesToEnd.forEach((line) => {
          if (line.points.length > 1) {
            isotherms.push({ temp, points: [...line.points] });
          }
        });

        activeLines = linesToKeep;
        return;
      }

      // Group close heights together
      heightsAtTemp.sort((a, b) => a - b);
      const groupedHeights: number[] = [];
      let currentGroup: number[] = [heightsAtTemp[0]];

      for (let i = 1; i < heightsAtTemp.length; i++) {
        if (heightsAtTemp[i] - currentGroup[0] < heightThreshold) {
          currentGroup.push(heightsAtTemp[i]);
        } else {
          // For each group, keep both the minimum and maximum heights if they're different enough
          const minHeight = Math.min(...currentGroup);
          const maxHeight = Math.max(...currentGroup);
          if (maxHeight - minHeight > heightThreshold / 2) {
            groupedHeights.push(minHeight, maxHeight);
          } else {
            // Use average for very close points
            groupedHeights.push(
              currentGroup.reduce((a, b) => a + b) / currentGroup.length,
            );
          }
          currentGroup = [heightsAtTemp[i]];
        }
      }
      // Add the last group
      if (currentGroup.length > 0) {
        const minHeight = Math.min(...currentGroup);
        const maxHeight = Math.max(...currentGroup);
        if (maxHeight - minHeight > heightThreshold / 2) {
          groupedHeights.push(minHeight, maxHeight);
        } else {
          groupedHeights.push(
            currentGroup.reduce((a, b) => a + b) / currentGroup.length,
          );
        }
      }

      // Try to continue each active line
      const newActiveLines: typeof activeLines = [];
      const usedHeights = new Set<number>();

      // First, try to continue existing lines
      activeLines.forEach((line) => {
        const lastColIndex = line.points[line.points.length - 1].x;
        if (colIndex - lastColIndex <= maxStepDistance) {
          const lastHeight = line.points[line.points.length - 1].y;
          let bestMatch = groupedHeights[0];
          let minDiff = Math.abs(bestMatch - lastHeight);

          // Find closest unused height
          groupedHeights.forEach((height) => {
            if (!usedHeights.has(height)) {
              const diff = Math.abs(height - lastHeight);
              if (diff < minDiff) {
                minDiff = diff;
                bestMatch = height;
              }
            }
          });

          // Adjust the logic for continuing lines
          if (minDiff < heightThreshold * 4) {
            // More lenient height difference threshold
            line.points.push({ x: colIndex, y: bestMatch });
            usedHeights.add(bestMatch);
            newActiveLines.push(line);
          } else {
            // End this line and start a new one
            if (line.points.length > 1) {
              isotherms.push({ temp, points: [...line.points] });
            }
          }
        } else {
          // End line if it exceeds maxStepDistance
          if (line.points.length > 1) {
            isotherms.push({ temp, points: [...line.points] });
          }
        }
      });

      // Start new lines for remaining unused heights
      groupedHeights.forEach((height) => {
        if (!usedHeights.has(height)) {
          newActiveLines.push({
            points: [{ x: colIndex, y: height }],
          });
        }
      });

      activeLines = newActiveLines;
    });

    // Add any remaining active lines
    activeLines.forEach((line) => {
      if (line.points.length > 1) {
        isotherms.push({ temp, points: [...line.points] });
      }
    });
  }

  return isotherms;
};

export default function Meteogram({
  width,
  height,
  weatherData,
  margin = defaultMargin,
  useLocalTime = false,
  highlightCeilingCoverage = true,
  clampCloudCoverageAt50Pct = true,
  isLoading = false,
  showPressureLines = false,
  showWindBarbs = true,
  showIsothermLines = false,
  model,
}: MeteogramProps) {
  // Call all hooks first, before any conditional logic
  const [hoveredRect, setHoveredRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);

  const [frozenRect, setFrozenRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);

  // Debounced hover handler
  const handleHover = useCallback((date: Date, cloudCell: CloudCell) => {
    setHoveredRect({ date, cloudCell });
  }, []);

  // Memoize bounds calculations
  const bounds = useMemo(() => {
    const xMax = width - margin.left - margin.right;
    const yMax = height - margin.top - margin.bottom;
    return { xMax, yMax };
  }, [width, height, margin]);

  // Memoize scales with safe defaults when data is not available
  const scales = useMemo(() => {
    if (weatherData.length === 0) {
      return {
        dateScale: scaleTime({
          domain: [new Date(), new Date()],
          range: [0, bounds.xMax],
        }),
        mslScale: scaleLinear<number>({
          domain: [0, 20_000],
          range: [bounds.yMax, 0],
        }),
        pressureScale: scaleLinear<number>({
          domain: [250, 1000],
          range: [0, bounds.yMax],
        }),
        cloudScale: scaleLinear<number>({
          domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
          range: [0, 1],
          clamp: true,
        }),
      };
    }

    return {
      dateScale: scaleTime({
        domain: [weatherData[0].date, weatherData[weatherData.length - 1].date],
        range: [0, bounds.xMax],
      }),
      mslScale: scaleLinear<number>({
        domain: [0, 20_000],
        range: [bounds.yMax, 0],
      }),
      pressureScale: scaleLinear<number>({
        domain: [250, 1000],
        range: [0, bounds.yMax],
      }),
      cloudScale: scaleLinear<number>({
        domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
        range: [0, 1],
        clamp: true,
      }),
    };
  }, [bounds, weatherData, clampCloudCoverageAt50Pct]);

  // Memoize helper functions
  const getYForPressureMemo = useMemo(
    () => (cloud: CloudCell[], hpa: number) => {
      const level = cloud.find((c) => c.hpa === hpa);
      return level?.geopotentialFt || 0;
    },
    [],
  );

  // Memoize pressure levels with safe default - now with dynamic filtering
  const pressureLevels = useMemo(() => {
    if (weatherData.length === 0) return [];

    // Get all unique pressure levels from the first column that have valid data
    const firstColumnLevels = weatherData[0].cloud
      .filter(
        (cloud) =>
          cloud.hpa != null &&
          cloud.mslFtTop != null &&
          cloud.mslFtBottom != null &&
          cloud.cloudCoverage != null &&
          Number.isFinite(cloud.mslFtTop) &&
          Number.isFinite(cloud.mslFtBottom) &&
          Number.isFinite(cloud.cloudCoverage),
      )
      .map((cloud) => cloud.hpa);

    const validLevels = new Set(firstColumnLevels);

    // Verify these levels have valid data across all columns
    return Array.from(validLevels)
      .filter((hpa) =>
        weatherData.every((column) => {
          const cloud = column.cloud.find((c) => c.hpa === hpa);
          const isValid =
            cloud &&
            cloud.mslFtTop != null &&
            cloud.mslFtBottom != null &&
            cloud.cloudCoverage != null &&
            Number.isFinite(cloud.mslFtTop) &&
            Number.isFinite(cloud.mslFtBottom) &&
            Number.isFinite(cloud.cloudCoverage);
          return isValid;
        }),
      )
      .sort((a, b) => b - a); // Sort by pressure level descending
  }, [weatherData]);

  const barWidth = useMemo(
    () => (weatherData.length > 0 ? bounds.xMax / weatherData.length : 0),
    [bounds.xMax, weatherData.length],
  );

  // Memoize the main rendering components - moved before early return
  const renderCloudColumns = useMemo(() => {
    if (weatherData.length === 0) return null;

    return (
      <>
        {/* Base cloud rectangles */}
        {weatherData.map((d) => {
          const filteredClouds = d.cloud?.filter(
            (cloud) => cloud.hpa != null && pressureLevels.includes(cloud.hpa),
          );

          return (
            <Group
              key={`date-group-${d.date}`}
              left={formatNumber(scales.dateScale(d.date))}
              className="cloud-column"
            >
              {filteredClouds?.map((cloud) => {
                const isHovered =
                  (hoveredRect?.date === d.date &&
                    hoveredRect?.cloudCell.hpa === cloud.hpa) ||
                  (frozenRect?.date === d.date &&
                    frozenRect?.cloudCell.hpa === cloud.hpa);

                const coverage = clampCloudCoverageAt50Pct
                  ? Math.min(cloud.cloudCoverage, 50)
                  : cloud.cloudCoverage;

                const fillColor =
                  cloud.cloudCoverage > 50 && highlightCeilingCoverage
                    ? `rgba(200, 200, 200, ${formatNumber(scales.cloudScale(coverage))})`
                    : `rgba(255, 255, 255, ${formatNumber(scales.cloudScale(coverage))})`;

                return (
                  <rect
                    className={`cloud-cell cloud-cell-${cloud.hpa}`}
                    key={`cloud-${cloud.hpa}`}
                    x={formatNumber(0)}
                    y={formatNumber(scales.mslScale(cloud.mslFtTop))}
                    width={formatNumber(isHovered ? barWidth * 1.1 : barWidth)}
                    height={formatNumber(
                      scales.mslScale(cloud.mslFtBottom) -
                        scales.mslScale(cloud.mslFtTop),
                    )}
                    fill={fillColor}
                    stroke="transparent"
                    strokeWidth={0}
                    style={{ cursor: "default" }}
                    onMouseEnter={() => {
                      if (!frozenRect) {
                        handleHover(d.date, cloud);
                      }
                    }}
                    onMouseLeave={() => {
                      if (!frozenRect) {
                        setHoveredRect(null);
                      }
                    }}
                    onClick={(event: React.MouseEvent) => {
                      if (
                        (event.nativeEvent as PointerEvent).pointerType ===
                        "mouse"
                      ) {
                        if (frozenRect) {
                          setFrozenRect(null);
                          setHoveredRect({
                            date: d.date,
                            cloudCell: cloud,
                          });
                        } else {
                          setFrozenRect({
                            date: d.date,
                            cloudCell: cloud,
                          });
                          setHoveredRect(null);
                        }
                      }
                    }}
                  ></rect>
                );
              })}
            </Group>
          );
        })}

        {/* Freezing Levels */}
        {findFreezingPoints(weatherData).map(({ points }, lineIndex) => {
          const pathD = points.reduce((path, point, i) => {
            const x = formatNumber(scales.dateScale(weatherData[point.x].date));
            const y = formatNumber(scales.mslScale(point.y));
            if (i === 0) return `M ${x} ${y}`;
            return `${path} L ${x} ${y}`;
          }, "");

          return (
            <path
              className={`freezing-level freezing-level-${lineIndex + 1}`}
              key={`freezing-level-${lineIndex}`}
              d={pathD}
              stroke="#0066cc"
              strokeWidth={2}
              strokeDasharray="4,4"
              fill="none"
            ></path>
          );
        })}

        {/* Isotherm Lines */}
        {showIsothermLines &&
          findIsothermPoints(weatherData, 2, 500, model).map(
            ({ temp, points }, lineIndex) => {
              const pathD = points.reduce((path, point, i) => {
                const x = formatNumber(
                  scales.dateScale(weatherData[point.x].date),
                );
                const y = formatNumber(scales.mslScale(point.y));
                if (i === 0) return `M ${x} ${y}`;
                return `${path} L ${x} ${y}`;
              }, "");

              return (
                <g
                  key={`isotherm-${temp}-${formatNumber(points[0].y)}-${lineIndex}`}
                  className={`isotherm-group isotherm-${temp}`}
                >
                  <path
                    className="isotherm-line"
                    d={pathD}
                    stroke={getTemperatureColor(temp)}
                    strokeWidth={1}
                    strokeDasharray="4,4"
                    opacity={0.7}
                    fill="none"
                  ></path>
                  <text
                    className="isotherm-label"
                    x={formatNumber(
                      scales.dateScale(weatherData[points[0].x].date),
                    )}
                    y={formatNumber(scales.mslScale(points[0].y))}
                    dx="-2.5em"
                    dy="0.3em"
                    fontSize="10"
                    fill={getTemperatureColor(temp)}
                    pointerEvents="none"
                  >
                    {`${temp}°C`}
                  </text>
                </g>
              );
            },
          )}

        {/* Pressure Lines */}
        {showPressureLines &&
          pressureLevels.map((hpa) => {
            const points = weatherData.map((d) => {
              const cloud = d.cloud.find((c) => c.hpa === hpa);
              return {
                x: formatNumber(scales.dateScale(d.date)),
                y: formatNumber(scales.mslScale(cloud?.geopotentialFt || 0)),
              };
            });

            const pathD = points.reduce((path, point, i) => {
              if (i === 0) return `M ${point.x} ${point.y}`;
              return `${path} L ${point.x} ${point.y}`;
            }, "");

            return (
              <path
                className={`pressure-line pressure-line-${hpa}`}
                key={`pressure-line-${hpa}`}
                d={pathD}
                stroke="gray"
                strokeWidth={1}
                strokeDasharray="4,4"
                opacity={0.5}
                fill="none"
              ></path>
            );
          })}

        {/* Wind Barbs */}
        {showWindBarbs &&
          model &&
          MODEL_CONFIGS[model] &&
          weatherData
            .filter(
              (_, index) => index % MODEL_CONFIGS[model].windBarbStep === 0,
            )
            .map((d) =>
              d.cloud
                ?.filter(
                  (cloud, levelIndex) =>
                    pressureLevels.includes(cloud.hpa) && // Use our dynamic pressure levels
                    levelIndex %
                      MODEL_CONFIGS[model].windBarbPressureLevelStep ===
                      0 && // Only show wind barbs every N pressure levels
                    cloud.windSpeed != null &&
                    cloud.windDirection != null &&
                    cloud.geopotentialFt != null,
                )
                .map((cloud) => (
                  <g
                    key={`wind-barb-${d.date}-${cloud.hpa}`}
                    className={`wind-barb-group wind-barb-${cloud.hpa}`}
                  >
                    <MemoizedWindBarb
                      x={formatNumber(scales.dateScale(d.date) + barWidth / 2)}
                      y={formatNumber(scales.mslScale(cloud.geopotentialFt))}
                      speed={cloud.windSpeed}
                      direction={cloud.windDirection}
                    />
                  </g>
                )),
            )}

        {/* Highlight borders - rendered last to appear on top */}
        {weatherData.map((d) => (
          <Group
            key={`highlight-group-${d.date}`}
            left={formatNumber(scales.dateScale(d.date))}
            className="highlight-group"
          >
            {d.cloud.map((cloud) => {
              const isHovered =
                (hoveredRect?.date === d.date &&
                  hoveredRect?.cloudCell.hpa === cloud.hpa) ||
                (frozenRect?.date === d.date &&
                  frozenRect?.cloudCell.hpa === cloud.hpa);

              if (!isHovered) return null;

              return (
                <rect
                  className={`highlight-border highlight-border-${cloud.hpa}`}
                  key={`highlight-${cloud.hpa}`}
                  x={formatNumber(0)}
                  y={formatNumber(scales.mslScale(cloud.mslFtTop))}
                  width={formatNumber(barWidth * 1.1)}
                  height={formatNumber(
                    scales.mslScale(cloud.mslFtBottom) -
                      scales.mslScale(cloud.mslFtTop),
                  )}
                  fill="none"
                  stroke={black}
                  strokeWidth={1}
                  pointerEvents="none"
                ></rect>
              );
            })}
          </Group>
        ))}

        {/* Hover/Frozen Indicators */}
        {(hoveredRect || frozenRect) && (
          <>
            <line
              className="hover-line hover-line-vertical"
              x1={formatNumber(
                scales.dateScale((hoveredRect || frozenRect)!.date),
              )}
              x2={formatNumber(
                scales.dateScale((hoveredRect || frozenRect)!.date),
              )}
              y1={0}
              y2={formatNumber(bounds.yMax)}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            ></line>
            <line
              className="hover-line hover-line-horizontal"
              x1={0}
              x2={formatNumber(bounds.xMax)}
              y1={formatNumber(
                scales.mslScale(
                  (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
                ),
              )}
              y2={formatNumber(
                scales.mslScale(
                  (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
                ),
              )}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            ></line>
          </>
        )}
      </>
    );
  }, [
    weatherData,
    scales,
    barWidth,
    hoveredRect,
    frozenRect,
    highlightCeilingCoverage,
    showWindBarbs,
    showPressureLines,
    showIsothermLines,
    model,
    bounds,
    handleHover,
    clampCloudCoverageAt50Pct,
    pressureLevels,
  ]);

  // Early return after all hooks are called
  if (isLoading || weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  return (
    <svg
      width={formatNumber(width)}
      height={formatNumber(height)}
      className="meteogram"
    >
      <rect
        className="meteogram-background"
        x={formatNumber(0)}
        y={formatNumber(0)}
        width={formatNumber(width)}
        height={formatNumber(height)}
        fill={background}
        rx={14}
      />
      <Group
        top={formatNumber(margin.top)}
        left={formatNumber(margin.left)}
        className="meteogram-content"
      >
        {renderCloudColumns}
      </Group>
      {/* Add axes on top of cloud cells */}
      <g className="axes-group" style={{ zIndex: 10 }}>
        <TimeAxis
          left={formatNumber(margin.left)}
          top={formatNumber(bounds.yMax + margin.top)}
          scale={scales.dateScale}
          useLocalTime={useLocalTime}
          stroke={black}
          tickStroke={black}
        />
        <AxisLeft
          axisClassName="height-axis"
          left={formatNumber(margin.left)}
          top={formatNumber(margin.top)}
          scale={scales.mslScale}
          numTicks={10}
          tickFormat={(value) => formatNumber(Number(value)).toString()}
          stroke={black}
          tickStroke={black}
          tickLabelProps={{
            fill: black,
            fontSize: 11,
            textAnchor: "end",
          }}
        />
        {showPressureLines && (
          <AxisLeft
            axisClassName="pressure-axis"
            left={formatNumber(margin.left)}
            top={formatNumber(margin.top)}
            scale={scales.mslScale}
            stroke="none"
            tickStroke={black}
            tickValues={pressureLevels.map((hpa) =>
              formatNumber(getYForPressureMemo(weatherData[0].cloud, hpa)),
            )}
            tickFormat={(value) => {
              const cloud = weatherData[0].cloud.find(
                (c) =>
                  formatNumber(c.geopotentialFt) ===
                  formatNumber(Number(value)),
              );
              return cloud ? `${cloud.hpa}` : "";
            }}
            tickLength={6}
            tickLabelProps={{
              fill: black,
              fontSize: 11,
              textAnchor: "start",
              dx: "0.5em",
            }}
            orientation="right"
            label="hPa"
          />
        )}
      </g>
      {/* Render tooltip last to ensure it's always on top */}
      {(hoveredRect || frozenRect) && (
        <foreignObject
          x={formatNumber(
            Math.min(
              scales.dateScale((hoveredRect || frozenRect)!.date) +
                margin.left +
                10,
              bounds.xMax + margin.left - 210,
            ),
          )}
          y={formatNumber(
            Math.min(
              scales.mslScale((hoveredRect || frozenRect)!.cloudCell.mslFtTop) +
                margin.top -
                10,
              bounds.yMax + margin.top - 160,
            ),
          )}
          width="200"
          height="200"
        >
          <div
            style={{
              backgroundColor: "rgba(255,255,255,0.9)",
              padding: "8px",
              borderRadius: "4px",
              boxShadow: "0 1px 3px rgba(0,0,0,0.2)",
              pointerEvents: "none",
              fontSize: "12px",
              zIndex: 100,
            }}
          >
            <div>{`Time: ${
              useLocalTime
                ? (hoveredRect || frozenRect)!.date.toLocaleTimeString()
                : (hoveredRect || frozenRect)!.date.toUTCString().split(" ")[4]
            } ${useLocalTime ? "Local" : "UTC"}`}</div>
            {(hoveredRect || frozenRect)!.cloudCell.hpa != null && (
              <div>{`Pressure: ${hPaToInHg((hoveredRect || frozenRect)!.cloudCell.hpa)} inHg (${
                (hoveredRect || frozenRect)!.cloudCell.hpa
              } hPa)`}</div>
            )}
            {(hoveredRect || frozenRect)!.cloudCell.mslFt != null && (
              <div>{`MSL Height: ${formatNumber((hoveredRect || frozenRect)!.cloudCell.mslFt)} ft`}</div>
            )}
            {(hoveredRect || frozenRect)!.cloudCell.mslFtTop != null &&
              (hoveredRect || frozenRect)!.cloudCell.mslFtBottom != null && (
                <div>{`Height Range: ${formatNumber((hoveredRect || frozenRect)!.cloudCell.mslFtTop)} - ${formatNumber(
                  (hoveredRect || frozenRect)!.cloudCell.mslFtBottom,
                )} ft`}</div>
              )}
            {(hoveredRect || frozenRect)!.cloudCell.cloudCoverage != null && (
              <div>{`Cloud Cover: ${formatNumber((hoveredRect || frozenRect)!.cloudCell.cloudCoverage)}%`}</div>
            )}
            {(hoveredRect || frozenRect)!.cloudCell.temperature != null && (
              <div>{`Temperature: ${formatNumber((hoveredRect || frozenRect)!.cloudCell.temperature)}°C`}</div>
            )}
            {(hoveredRect || frozenRect)!.cloudCell.windSpeed != null && (
              <div>{`Wind Speed: ${kmhToKnots((hoveredRect || frozenRect)!.cloudCell.windSpeed)} kt`}</div>
            )}
            {(hoveredRect || frozenRect)!.cloudCell.windDirection != null && (
              <div>{`Wind Direction: ${formatNumber((hoveredRect || frozenRect)!.cloudCell.windDirection)}°`}</div>
            )}
          </div>
        </foreignObject>
      )}
    </svg>
  );
}
