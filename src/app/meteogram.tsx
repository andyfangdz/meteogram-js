"use client";

import React, { useState, useMemo, useCallback } from "react";
import { Group } from "@visx/group";
import { AxisBottom, AxisLeft } from "@visx/axis";
import { scaleLinear, scaleTime } from "@visx/scale";
import { utcFormat, timeFormat } from "@visx/vendor/d3-time-format";
import { CloudColumn, CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import LoadingSkeleton from "./loading-skeleton";
import WindBarb from "./components/wind-barb";
import { WIND_BARB_LEVELS, MODEL_CONFIGS } from "../config/weather";
import { WeatherModel } from "../types/weather";

const hPaToInHg = (hpa: number) => (hpa * 0.02953).toFixed(2);
// Convert km/h to knots
const kmhToKnots = (kmh: number) => (kmh * 0.539957).toFixed(0);

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

  if (isLoading || weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  // Memoize bounds calculations
  const bounds = useMemo(() => {
    const xMax = width - margin.left - margin.right;
    const yMax = height - margin.top - margin.bottom;
    return { xMax, yMax };
  }, [width, height, margin]);

  // Memoize scales
  const scales = useMemo(() => {
    const dateScale = scaleTime({
      domain: [weatherData[0].date, weatherData[weatherData.length - 1].date],
      range: [0, bounds.xMax],
    });

    const mslScale = scaleLinear<number>({
      domain: [0, 20_000],
    }).range([bounds.yMax, 0]);

    const pressureScale = scaleLinear<number>({
      domain: [250, 1000],
    }).range([0, bounds.yMax]);

    const cloudScale = scaleLinear<number>({
      domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
      range: [0, 1],
      clamp: true,
    });

    return { dateScale, mslScale, pressureScale, cloudScale };
  }, [bounds, weatherData, clampCloudCoverageAt50Pct]);

  // Memoize helper functions
  const getYForPressureMemo = useMemo(
    () => (cloud: CloudCell[], hpa: number) => {
      const level = cloud.find((c) => c.hpa === hpa);
      return level?.geopotentialFt || 0;
    },
    [],
  );

  // Memoize pressure levels
  const pressureLevels = useMemo(
    () => weatherData[0].cloud.map((c) => c.hpa),
    [weatherData],
  );

  const barWidth = bounds.xMax / weatherData.length;

  // Memoize the main rendering components
  const renderCloudColumns = useMemo(() => {
    return (
      <>
        {weatherData.map((d) => (
          <Group key={`date-group-${d.date}`} left={scales.dateScale(d.date)}>
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
                  ? `rgba(200, 200, 200, ${scales.cloudScale(coverage)})`
                  : `rgba(255, 255, 255, ${scales.cloudScale(coverage)})`;

              return (
                <rect
                  key={`cloud-${cloud.hpa}`}
                  x={0}
                  y={scales.mslScale(cloud.mslFtTop)}
                  width={isHovered ? barWidth * 1.1 : barWidth}
                  height={
                    scales.mslScale(cloud.mslFtBottom) -
                    scales.mslScale(cloud.mslFtTop)
                  }
                  fill={fillColor}
                  stroke={isHovered ? black : "transparent"}
                  strokeWidth={isHovered ? 1 : 0}
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
                    // Only enable freeze for mouse events, not touch
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
                  style={{ cursor: "default" }}
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
              d={`M ${scales.dateScale(d.date)} ${scales.mslScale(currentLevel)} L ${scales.dateScale(weatherData[i + 1].date)} ${scales.mslScale(nextLevel)}`}
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
              x: scales.dateScale(d.date),
              y: scales.mslScale(d.cloud[pressureIndex].geopotentialFt),
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
                    x={scales.dateScale(d.date) + barWidth / 2}
                    y={scales.mslScale(cloud.geopotentialFt)}
                    speed={parseFloat(kmhToKnots(cloud.windSpeed))}
                    direction={cloud.windDirection}
                  />
                )),
            )}

        {/* Hover/Frozen Indicators */}
        {(hoveredRect || frozenRect) && (
          <>
            <line
              x1={scales.dateScale((hoveredRect || frozenRect)!.date)}
              x2={scales.dateScale((hoveredRect || frozenRect)!.date)}
              y1={0}
              y2={bounds.yMax}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <line
              x1={0}
              x2={bounds.xMax}
              y1={scales.mslScale(
                (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
              )}
              y2={scales.mslScale(
                (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
              )}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <foreignObject
              x={
                hoveredRect || frozenRect
                  ? (() => {
                      const cursorX = scales.dateScale(
                        (hoveredRect || frozenRect)!.date,
                      );
                      if (cursorX - 210 < 0) {
                        return Math.min(cursorX + 10, bounds.xMax - 200);
                      }
                      return Math.min(cursorX - 210, bounds.xMax - 200);
                    })()
                  : 0
              }
              y={
                hoveredRect || frozenRect
                  ? (() => {
                      const tooltipHeight = 160;
                      const cursorY = scales.mslScale(
                        (hoveredRect || frozenRect)!.cloudCell.mslFtTop,
                      );
                      const spaceBelow = bounds.yMax - cursorY;

                      if (spaceBelow < tooltipHeight / 3) {
                        return Math.max(
                          margin.top,
                          cursorY - tooltipHeight - 10,
                        );
                      }
                      return Math.min(
                        cursorY + 10,
                        bounds.yMax + margin.top - tooltipHeight,
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
                <div>{`Date: ${(hoveredRect || frozenRect)!.date.toLocaleDateString()}`}</div>
                <div>{`Time: ${(hoveredRect || frozenRect)!.date.toLocaleTimeString()}`}</div>
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

  return (
    <svg width={width} height={height}>
      <rect
        x={0}
        y={0}
        width={width}
        height={height}
        fill={background}
        rx={14}
      />
      <AxisBottom
        left={margin.left}
        top={bounds.yMax + margin.top}
        scale={scales.dateScale}
        tickFormat={(value) => {
          if (value instanceof Date) {
            return useLocalTime
              ? timeFormat("%d%H")(value)
              : utcFormat("%d%HZ")(value);
          }
          const date = new Date(Number(value));
          return useLocalTime
            ? timeFormat("%d%H")(date)
            : utcFormat("%d%HZ")(date);
        }}
        stroke={black}
        tickStroke={black}
        tickLabelProps={{
          fill: black,
          fontSize: 11,
          textAnchor: "middle",
        }}
      />
      <AxisLeft
        left={margin.left}
        top={margin.top}
        scale={scales.mslScale}
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
          left={margin.left}
          top={margin.top}
          scale={scales.mslScale}
          stroke="none"
          tickStroke={black}
          tickValues={pressureLevels.map((hpa) =>
            getYForPressureMemo(weatherData[0].cloud, hpa),
          )}
          tickFormat={(value) => {
            const cloud = weatherData[0].cloud.find(
              (c) => c.geopotentialFt === value,
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
      <Group top={margin.top} left={margin.left}>
        {renderCloudColumns}
      </Group>
    </svg>
  );
}
