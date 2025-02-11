"use client";

import React, { useState } from "react";
import { Group } from "@visx/group";
import { AxisBottom, AxisLeft } from "@visx/axis";
import { scaleLinear, scaleTime } from "@visx/scale";
import { utcFormat, timeFormat } from "@visx/vendor/d3-time-format";
import { CloudColumn, CloudCell } from "../types/weather";
import { FEET_PER_METER } from "../config/weather";
import LoadingSkeleton from "./loading-skeleton";

const hPaToInHg = (hpa: number) => (hpa * 0.02953).toFixed(2);

// Add a helper to get y position for a pressure level
const getYForPressure = (cloud: CloudCell[], hpa: number) => {
  const level = cloud.find((c) => c.hpa === hpa);
  return level?.geopotentialFt || 0;
};

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
  showFreezingLevels?: boolean;
};

const black = "#000000";
const background = "#87CEEB";
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

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

  // For each level in the first column
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

  return matches;
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
  showFreezingLevels = true,
}: MeteogramProps) {
  const [hoveredRect, setHoveredRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);
  const [frozenRect, setFrozenRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);

  if (isLoading || weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  // bounds
  const xMax = width - margin.left - margin.right;
  const yMax = height - margin.top - margin.bottom;

  // scales
  const dateScale = scaleTime<number>({
    domain: [weatherData[0].date, weatherData[weatherData.length - 1].date],
  }).range([0, xMax]);

  const mslScale = scaleLinear<number>({
    domain: [0, 20_000],
  }).range([yMax, 0]);

  const pressureScale = scaleLinear<number>({
    domain: [250, 1000], // HPA_LEVELS range from config
  }).range([0, yMax]);

  const cloudScale = scaleLinear<number>({
    domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
  }).range([0, 1]);

  const barWidth = xMax / weatherData.length;

  // Get all pressure levels from the first column
  const pressureLevels = weatherData[0].cloud.map((c) => c.hpa);

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
        top={yMax + margin.top}
        tickFormat={useLocalTime ? timeFormat("%d%H") : utcFormat("%d%HZ")}
        scale={dateScale}
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
        scale={mslScale}
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
          scale={mslScale}
          stroke="none"
          tickStroke={black}
          tickValues={pressureLevels.map((hpa) =>
            getYForPressure(weatherData[0].cloud, hpa),
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
        {weatherData.map((d) => (
          <Group key={`date-group-${d.date}`} left={dateScale(d.date)}>
            {d.cloud.map((cloud) => {
              const isHovered =
                (hoveredRect?.date === d.date &&
                  hoveredRect?.cloudCell.hpa === cloud.hpa) ||
                (frozenRect?.date === d.date &&
                  frozenRect?.cloudCell.hpa === cloud.hpa);
              const fillColor =
                cloud.cloudCoverage > 50 && highlightCeilingCoverage
                  ? `rgba(200, 200, 200, ${cloudScale(cloud.cloudCoverage)})`
                  : `rgba(255, 255, 255, ${cloudScale(cloud.cloudCoverage)})`;
              return (
                <rect
                  key={`cloud-group-${d.date}-${cloud.hpa}`}
                  x={0}
                  y={mslScale(cloud.mslFtTop)}
                  width={isHovered ? barWidth * 1.1 : barWidth}
                  height={
                    mslScale(cloud.mslFtBottom) - mslScale(cloud.mslFtTop)
                  }
                  fill={fillColor}
                  stroke={isHovered ? black : "transparent"}
                  strokeWidth={isHovered ? 1 : 0}
                  onMouseEnter={() => {
                    if (!frozenRect) {
                      setHoveredRect({
                        date: d.date,
                        cloudCell: cloud,
                      });
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
        {showFreezingLevels &&
          weatherData.map((d, i) => {
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
                d={`M ${dateScale(d.date)} ${mslScale(currentLevel)} L ${dateScale(weatherData[i + 1].date)} ${mslScale(nextLevel)}`}
                stroke="#0066cc"
                strokeWidth={2}
                strokeDasharray="4,4"
                fill="none"
              />
            ));
          })}
        {showPressureLines &&
          weatherData[0].cloud.map((_, pressureIndex) => {
            // Create a path for each pressure level
            const points = weatherData.map((d) => ({
              x: dateScale(d.date),
              y: mslScale(d.cloud[pressureIndex].geopotentialFt),
            }));

            // Create SVG path string
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
        {(hoveredRect || frozenRect) && (
          <>
            <line
              x1={dateScale((hoveredRect || frozenRect)!.date)}
              x2={dateScale((hoveredRect || frozenRect)!.date)}
              y1={0}
              y2={yMax}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <line
              x1={0}
              x2={xMax}
              y1={mslScale((hoveredRect || frozenRect)!.cloudCell.mslFt)}
              y2={mslScale((hoveredRect || frozenRect)!.cloudCell.mslFt)}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <foreignObject
              x={
                hoveredRect || frozenRect
                  ? (() => {
                      const cursorX = dateScale(
                        (hoveredRect || frozenRect)!.date,
                      ) as number;
                      // If we're close to the left edge, show tooltip on the right side of the cursor
                      if (cursorX - 210 < 0) {
                        return Math.min(cursorX + 10, xMax - 200);
                      }
                      // Otherwise show it on the left side, but don't let it overflow right edge
                      return Math.min(cursorX - 210, xMax - 200);
                    })()
                  : 0
              }
              y={
                hoveredRect || frozenRect
                  ? (() => {
                      const tooltipHeight = 160; // Approximate height of tooltip
                      const cursorY = mslScale(
                        (hoveredRect || frozenRect)!.cloudCell.mslFt,
                      );
                      const spaceBelow = yMax - cursorY;

                      // If there's less than 1/3 of the tooltip height space below, show above
                      if (spaceBelow < tooltipHeight / 3) {
                        return Math.max(
                          margin.top,
                          cursorY - tooltipHeight - 10,
                        );
                      }
                      // Otherwise show below
                      return Math.min(
                        cursorY + 10,
                        yMax + margin.top - tooltipHeight,
                      );
                    })()
                  : 0
              }
              width={200}
              height={160}
              style={{
                pointerEvents: "none",
              }}
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
                <div>{`Geopotential Height: ${(hoveredRect || frozenRect)!.cloudCell.geopotentialFt.toFixed(2)} ft`}</div>
                {showPressureLines && (
                  <div>{`Pressure: ${(hoveredRect || frozenRect)!.cloudCell.hpa} hPa (${hPaToInHg((hoveredRect || frozenRect)!.cloudCell.hpa)} inHg)`}</div>
                )}
                <div>{`Cloud Cover: ${(hoveredRect || frozenRect)!.cloudCell.cloudCoverage.toFixed(2)}%`}</div>
                <div>{`Temperature: ${(hoveredRect || frozenRect)!.cloudCell.temperature.toFixed(1)}°C`}</div>
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
      </Group>
    </svg>
  );
}
