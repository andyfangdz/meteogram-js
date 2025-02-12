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

const hPaToInHg = (hpa: number) => (hpa * 0.02953).toFixed(2);
// Convert km/h to knots
const kmhToKnots = (kmh: number) => (kmh * 0.539957).toFixed(0);

// Add this helper function at the top with other constants
const formatNumber = (num: number) => Number(num.toFixed(4));

// Helper function to find freezing levels in a cloud column
const findFreezingLevels = (
  cloudColumn: CloudCell[],
  groundTemp: number,
): number[] => {
  const freezingLevels: number[] = [];

  // Sort the cloud column by pressure (height) from ground up
  const sortedColumn = [...cloudColumn].sort((a, b) => b.hpa - a.hpa);

  // Check if ground level is already below freezing
  if (groundTemp <= 0) {
    freezingLevels.push(0); // Start from ground level
  }
  // Check if there's a crossing point between ground and first pressure level
  else if (groundTemp > 0 && sortedColumn[0].temperature <= 0) {
    // Linear interpolation between ground (2m) and first pressure level
    const t1 = groundTemp;
    const t2 = sortedColumn[0].temperature;
    const h1 = 2 * FEET_PER_METER; // 2 meters above ground
    const h2 = sortedColumn[0].mslFt;
    const freezingLevel = h1 + ((0 - t1) * (h2 - h1)) / (t2 - t1);
    freezingLevels.push(freezingLevel);
  }

  // Find crossing points between pressure levels
  for (let i = 0; i < sortedColumn.length - 1; i++) {
    if (
      (sortedColumn[i].temperature > 0 &&
        sortedColumn[i + 1].temperature <= 0) ||
      (sortedColumn[i].temperature <= 0 && sortedColumn[i + 1].temperature > 0)
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
  model: WeatherModel;
};

const black = "#000000";
const background = "#87CEEB";
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

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

  // Memoize pressure levels with safe default
  const pressureLevels = useMemo(
    () =>
      weatherData.length > 0 ? weatherData[0].cloud.map((c) => c.hpa) : [],
    [weatherData],
  );

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
        {weatherData.map((d) => (
          <Group
            key={`date-group-${d.date}`}
            left={formatNumber(scales.dateScale(d.date))}
          >
            {d.cloud.map((cloud) => {
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
                />
              );
            })}
          </Group>
        ))}

        {/* Freezing Levels */}
        {weatherData.map((d, i) => {
          if (i === weatherData.length - 1) return null;

          const currentLevels = findFreezingLevels(d.cloud, d.groundTemp);
          const nextLevels = findFreezingLevels(
            weatherData[i + 1].cloud,
            weatherData[i + 1].groundTemp,
          );
          const matches = matchFreezingLevels(currentLevels, nextLevels);

          return matches.map(([currentLevel, nextLevel], levelIndex) => (
            <path
              key={`freezing-level-${d.date}-${levelIndex}`}
              d={`M ${formatNumber(scales.dateScale(d.date))} ${formatNumber(scales.mslScale(currentLevel))} L ${formatNumber(scales.dateScale(weatherData[i + 1].date))} ${formatNumber(scales.mslScale(nextLevel))}`}
              stroke="#0066cc"
              strokeWidth={2}
              strokeDasharray="4,4"
              fill="none"
            />
          ));
        })}

        {/* Pressure Lines */}
        {showPressureLines &&
          weatherData[0].cloud.map((_, pressureIndex) => {
            const points = weatherData.map((d) => ({
              x: formatNumber(scales.dateScale(d.date)),
              y: formatNumber(
                scales.mslScale(d.cloud[pressureIndex].geopotentialFt),
              ),
            }));

            const pathD = points.reduce((path, point, i) => {
              if (i === 0) return `M ${point.x} ${point.y}`;
              return `${path} L ${point.x} ${point.y}`;
            }, "");

            return (
              <path
                key={`pressure-line-${pressureIndex}`}
                d={pathD}
                stroke="gray"
                strokeWidth={1}
                strokeDasharray="4,4"
                opacity={0.5}
                fill="none"
              />
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
                .filter((cloud) => WIND_BARB_LEVELS.includes(cloud.hpa))
                .map((cloud) => (
                  <MemoizedWindBarb
                    key={`wind-barb-${d.date}-${cloud.hpa}`}
                    x={formatNumber(scales.dateScale(d.date) + barWidth / 2)}
                    y={formatNumber(scales.mslScale(cloud.geopotentialFt))}
                    speed={parseFloat(kmhToKnots(cloud.windSpeed))}
                    direction={cloud.windDirection}
                  />
                )),
            )}

        {/* Highlight borders - rendered last to appear on top */}
        {weatherData.map((d) => (
          <Group
            key={`highlight-group-${d.date}`}
            left={formatNumber(scales.dateScale(d.date))}
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
                />
              );
            })}
          </Group>
        ))}

        {/* Hover/Frozen Indicators */}
        {(hoveredRect || frozenRect) && (
          <>
            <line
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
            />
            <line
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
            />
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
    model,
    bounds,
    handleHover,
  ]);

  // Early return after all hooks are called
  if (isLoading || weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  return (
    <svg width={formatNumber(width)} height={formatNumber(height)}>
      <rect
        x={formatNumber(0)}
        y={formatNumber(0)}
        width={formatNumber(width)}
        height={formatNumber(height)}
        fill={background}
        rx={14}
      />
      <Group top={formatNumber(margin.top)} left={formatNumber(margin.left)}>
        {renderCloudColumns}
      </Group>
      {/* Add axes on top of cloud cells */}
      <g style={{ zIndex: 10 }}>
        <TimeAxis
          left={formatNumber(margin.left)}
          top={formatNumber(bounds.yMax + margin.top)}
          scale={scales.dateScale}
          useLocalTime={useLocalTime}
          stroke={black}
          tickStroke={black}
        />
        <AxisLeft
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
          x={
            hoveredRect || frozenRect
              ? (() => {
                  const cursorX = formatNumber(
                    scales.dateScale((hoveredRect || frozenRect)!.date),
                  );
                  if (cursorX - 210 < 0) {
                    return formatNumber(
                      Math.min(
                        cursorX + margin.left + 10,
                        bounds.xMax + margin.left - 200,
                      ),
                    );
                  }
                  return formatNumber(
                    Math.min(
                      cursorX + margin.left - 210,
                      bounds.xMax + margin.left - 200,
                    ),
                  );
                })()
              : 0
          }
          y={
            hoveredRect || frozenRect
              ? (() => {
                  const tooltipHeight = 160;
                  const cursorY = formatNumber(
                    scales.mslScale(
                      (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
                    ),
                  );
                  const spaceBelow = bounds.yMax - cursorY;

                  if (spaceBelow < tooltipHeight / 3) {
                    return formatNumber(
                      Math.max(
                        margin.top,
                        cursorY + margin.top - tooltipHeight - 10,
                      ),
                    );
                  }
                  return formatNumber(
                    Math.min(
                      cursorY + margin.top + 10,
                      bounds.yMax + margin.top - tooltipHeight,
                    ),
                  );
                })()
              : 0
          }
          width={200}
          height={160}
          style={{ pointerEvents: "none" }}
        >
          <div
            style={{
              backgroundColor: "white",
              border: `1px solid ${frozenRect ? "#666" : black}`,
              borderRadius: "4px",
              padding: "4px",
              fontSize: "10px",
              pointerEvents: "none",
              boxShadow: frozenRect ? "0 2px 4px rgba(0,0,0,0.2)" : "none",
            }}
          >
            <div>{`Date: ${(hoveredRect || frozenRect)!.date.toUTCString().split(" ").slice(0, 4).join(" ")}`}</div>
            <div>{`Time: ${(hoveredRect || frozenRect)!.date.toUTCString().split(" ")[4]}`}</div>
            <div>{`MSL Height: ${(hoveredRect || frozenRect)!.cloudCell.mslFt.toFixed(2)} ft`}</div>
            <div>{`Height Range: ${(hoveredRect || frozenRect)!.cloudCell.mslFtTop.toFixed(2)} - ${(hoveredRect || frozenRect)!.cloudCell.mslFtBottom.toFixed(2)} ft`}</div>
            {showPressureLines && (
              <div>{`Pressure: ${(hoveredRect || frozenRect)!.cloudCell.hpa} hPa (${hPaToInHg((hoveredRect || frozenRect)!.cloudCell.hpa)} inHg)`}</div>
            )}
            <div>{`Cloud Cover: ${(hoveredRect || frozenRect)!.cloudCell.cloudCoverage.toFixed(2)}%`}</div>
            <div>{`Temperature: ${(hoveredRect || frozenRect)!.cloudCell.temperature.toFixed(1)}°C`}</div>
            <div>{`Wind Speed: ${kmhToKnots((hoveredRect || frozenRect)!.cloudCell.windSpeed)} kt`}</div>
            <div>{`Wind Direction: ${(hoveredRect || frozenRect)!.cloudCell.windDirection.toFixed(0)}°`}</div>
            {frozenRect && (
              <div
                style={{
                  marginTop: "4px",
                  color: "#666",
                  fontStyle: "italic",
                }}
              >
                Click again to unfreeze
              </div>
            )}
          </div>
        </foreignObject>
      )}
    </svg>
  );
}
