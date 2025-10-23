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
  showIsotachLines?: boolean;
  model: WeatherModel;
  elevationFt: number | null;
};

const black = "#000000";
const background = "#87CEEB";
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

// Memoized to prevent re-renders when parent updates without prop changes.
// Note: weatherData reference changes only when new data is fetched, which correctly triggers re-render.
const Meteogram = React.memo(function Meteogram({
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
  showIsotachLines = false,
  model,
  elevationFt,
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

  // Memoize pressure levels with optimized O(n) algorithm
  const pressureLevels = useMemo(() => {
    if (weatherData.length === 0) return [];

    // Count how many columns have each valid hpa level
    const hpaCount = new Map<number, number>();

    for (const column of weatherData) {
      const validHpasInColumn = new Set<number>();

      for (const cloud of column.cloud) {
        // Check if this cloud cell has valid data
        if (
          cloud.hpa != null &&
          cloud.mslFtTop != null &&
          cloud.mslFtBottom != null &&
          cloud.cloudCoverage != null &&
          Number.isFinite(cloud.mslFtTop) &&
          Number.isFinite(cloud.mslFtBottom) &&
          Number.isFinite(cloud.cloudCoverage)
        ) {
          validHpasInColumn.add(cloud.hpa);
        }
      }

      // Increment count for each valid hpa in this column
      for (const hpa of validHpasInColumn) {
        hpaCount.set(hpa, (hpaCount.get(hpa) || 0) + 1);
      }
    }

    // Only keep hpa levels that appear in ALL columns
    const validLevels = Array.from(hpaCount.entries())
      .filter(([_, count]) => count === weatherData.length)
      .map(([hpa, _]) => hpa)
      .sort((a, b) => b - a);

    return validLevels;
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
        {/* Ground Elevation */}
        {elevationFt !== null && (
          <rect
            className="ground-elevation"
            x={0}
            y={formatNumber(scales.mslScale(elevationFt))}
            width={formatNumber(bounds.xMax)}
            height={formatNumber(bounds.yMax - scales.mslScale(elevationFt))}
            fill="#8B4513" // Brown color
            opacity={0.5} // Make it semi-transparent
            pointerEvents="none" // Don't block mouse events
          />
        )}

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
          showIsotachLines={showIsotachLines}
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
});

export default Meteogram;
