"use client";

import React, { useState, useMemo, useCallback } from "react";
import { Group } from "@visx/group";
import { AxisLeft } from "@visx/axis";
import { CloudColumn, CloudCell } from "../../types/weather";
import LoadingSkeleton from "./loading-skeleton";
import TimeAxis from "./time-axis";
import { WeatherModel } from "../../types/weather";
import { useMeteogramScales } from "../../hooks/useMeteogramScales";
import CloudColumns from "./cloud-columns";
import WeatherLines from "./weather-lines";
import MeteogramTooltip from "./meteogram-tooltip";
import PressureLines from "./pressure-lines";
import HoverIndicators from "./hover-indicators";
import { formatNumber } from "../../utils/meteogram";

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
  // State
  const [hoveredRect, setHoveredRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);

  const [frozenRect, setFrozenRect] = useState<{
    date: Date;
    cloudCell: CloudCell;
  } | null>(null);

  // Memoized calculations
  const bounds = useMemo(() => {
    const xMax = width - margin.left - margin.right;
    const yMax = height - margin.top - margin.bottom;
    return { xMax, yMax };
  }, [width, height, margin]);

  const scales = useMeteogramScales(
    weatherData,
    bounds,
    clampCloudCoverageAt50Pct,
  );

  const barWidth = useMemo(
    () => (weatherData.length > 0 ? bounds.xMax / weatherData.length : 0),
    [bounds.xMax, weatherData.length],
  );

  // Memoize pressure levels
  const pressureLevels = useMemo(() => {
    if (weatherData.length === 0) return [];

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
      .sort((a, b) => b - a);
  }, [weatherData]);

  // Handlers
  const handleHover = useCallback(
    (date: Date | null, cloudCell: CloudCell | null) => {
      if (date && cloudCell) {
        setHoveredRect({ date, cloudCell });
      } else {
        setHoveredRect(null);
      }
    },
    [],
  );

  const handleFreezeChange = useCallback(
    (rect: { date: Date; cloudCell: CloudCell } | null) => {
      setFrozenRect(rect);
    },
    [],
  );

  // Early return for loading state
  if (isLoading || weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  return (
    <svg
      width={formatNumber(width)}
      height={formatNumber(height)}
      className="meteogram"
      onMouseLeave={() => {
        if (!frozenRect) {
          setHoveredRect(null);
        }
      }}
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
        <CloudColumns
          weatherData={weatherData}
          scales={scales}
          barWidth={barWidth}
          pressureLevels={pressureLevels}
          highlightCeilingCoverage={highlightCeilingCoverage}
          clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
          showWindBarbs={showWindBarbs}
          model={model}
          frozenRect={frozenRect}
          onHover={handleHover}
          onFreezeChange={handleFreezeChange}
        />
        <WeatherLines
          weatherData={weatherData}
          scales={scales}
          showIsothermLines={showIsothermLines}
          model={model}
        />
        {showPressureLines && (
          <PressureLines
            weatherData={weatherData}
            scales={scales}
            pressureLevels={pressureLevels}
          />
        )}
        {(hoveredRect || frozenRect) && (
          <HoverIndicators
            date={(hoveredRect || frozenRect)!.date}
            cloudCell={(hoveredRect || frozenRect)!.cloudCell}
            scales={scales}
            bounds={bounds}
          />
        )}
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
        <g className="axis-units">
          <line
            x1={formatNumber(margin.left)}
            x2={formatNumber(margin.left)}
            y1={formatNumber(bounds.yMax + margin.top)}
            y2={formatNumber(bounds.yMax + margin.top + 25)}
            stroke={black}
          />
          <text
            x={formatNumber(margin.left - 5)}
            y={formatNumber(bounds.yMax + margin.top + 30)}
            fontSize={11}
            fill={black}
            textAnchor="end"
          >
            ft MSL
          </text>
          <text
            x={formatNumber(margin.left + 5)}
            y={formatNumber(bounds.yMax + margin.top + 30)}
            fontSize={11}
            fill={black}
            textAnchor="start"
          >
            hPa
          </text>
        </g>
      </g>
      {/* Render tooltip */}
      {(hoveredRect || frozenRect) && (
        <MeteogramTooltip
          date={(hoveredRect || frozenRect)!.date}
          cloudCell={(hoveredRect || frozenRect)!.cloudCell}
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
          useLocalTime={useLocalTime}
          frozen={frozenRect !== null}
        />
      )}
    </svg>
  );
}
