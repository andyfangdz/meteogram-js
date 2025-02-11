"use client";

import React, { useState } from "react";
import { Group } from "@visx/group";
import { AxisBottom, AxisLeft } from "@visx/axis";
import { scaleLinear, scaleTime } from "@visx/scale";
import { utcFormat, timeFormat } from "@visx/vendor/d3-time-format";
import { CloudColumn, CloudCell } from "../types/weather";
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
}: MeteogramProps) {
  const [hoveredRect, setHoveredRect] = useState<{
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
                hoveredRect?.date === d.date &&
                hoveredRect?.cloudCell.hpa === cloud.hpa;
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
                  onMouseEnter={() =>
                    setHoveredRect({
                      date: d.date,
                      cloudCell: cloud,
                    })
                  }
                  onMouseLeave={() => setHoveredRect(null)}
                />
              );
            })}
          </Group>
        ))}
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
        {hoveredRect && (
          <>
            <line
              x1={dateScale(hoveredRect.date)}
              x2={dateScale(hoveredRect.date)}
              y1={0}
              y2={yMax}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <line
              x1={0}
              x2={xMax}
              y1={mslScale(hoveredRect.cloudCell.mslFt)}
              y2={mslScale(hoveredRect.cloudCell.mslFt)}
              stroke={black}
              strokeWidth={1}
              pointerEvents="none"
            />
            <foreignObject
              x={
                hoveredRect
                  ? (dateScale(hoveredRect.date) as number) - 210 > 0
                    ? (dateScale(hoveredRect.date) as number) - 210
                    : (dateScale(hoveredRect.date) as number) + 10
                  : 0
              }
              y={hoveredRect ? mslScale(hoveredRect.cloudCell.mslFt) - 30 : 0}
              width={200}
              height={200}
              style={{
                pointerEvents: "none",
              }}
            >
              <div
                style={{
                  backgroundColor: "white",
                  border: "1px solid black",
                  borderRadius: "4px",
                  padding: "4px",
                  fontSize: "10px",
                  pointerEvents: "none",
                }}
              >
                <div>{`Date: ${hoveredRect.date.toLocaleDateString()}`}</div>
                <div>{`Time: ${hoveredRect.date.toLocaleTimeString()}`}</div>
                <div>{`MSL Height: ${hoveredRect.cloudCell.mslFt.toFixed(2)} ft`}</div>
                <div>{`Geopotential Height: ${hoveredRect.cloudCell.geopotentialFt.toFixed(2)} ft`}</div>
                {showPressureLines && (
                  <div>{`Pressure: ${hoveredRect.cloudCell.hpa} hPa (${hPaToInHg(hoveredRect.cloudCell.hpa)} inHg)`}</div>
                )}
                <div>{`Cloud Cover: ${hoveredRect.cloudCell.cloudCoverage.toFixed(2)}%`}</div>
              </div>
            </foreignObject>
          </>
        )}
      </Group>
    </svg>
  );
}
