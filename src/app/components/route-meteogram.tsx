"use client";

import React, { useState, useMemo, useCallback } from "react";
import { Group } from "@visx/group";
import { AxisLeft, AxisBottom } from "@visx/axis";
import type { CloudColumn, CloudCell, RouteWaypoint, WeatherModel } from "../../types/weather";
import { useRouteScales } from "../../hooks/useRouteScales";
import { MODEL_CONFIGS } from "../../config/weather";
import { formatNumber } from "../../utils/meteogram";
import TerrainProfile from "./terrain-profile";
import RouteCloudColumns from "./route-cloud-columns";
import RouteWeatherLines from "./route-weather-lines";
import RoutePressureLines from "./route-pressure-lines";
import AltitudeLine from "./altitude-line";
import WaypointMarkers from "./waypoint-markers";
import MeteogramTooltip from "./meteogram-tooltip";

const black = "#000000";
const background = "#87CEEB";
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

export interface RouteMeteogramProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  elevations: Array<number | null>;
  cruiseAltitudeFt: number;
  width: number;
  height: number;
  margin?: { top: number; right: number; bottom: number; left: number };
  // Visualization preferences
  useLocalTime?: boolean;
  highlightCeilingCoverage?: boolean;
  clampCloudCoverageAt50Pct?: boolean;
  showPressureLines?: boolean;
  showWindBarbs?: boolean;
  showIsothermLines?: boolean;
  showIsotachLines?: boolean;
  showDewPointDepressionLines?: boolean;
  model: WeatherModel;
  elevationFt?: number | null;
}

const RouteMeteogram = React.memo(function RouteMeteogram({
  crossSectionData,
  waypoints,
  elevations,
  cruiseAltitudeFt,
  width,
  height,
  margin = defaultMargin,
  useLocalTime = false,
  highlightCeilingCoverage = true,
  clampCloudCoverageAt50Pct = true,
  showPressureLines = false,
  showWindBarbs = true,
  showIsothermLines = false,
  showIsotachLines = false,
  showDewPointDepressionLines = true,
  model,
  elevationFt = null,
}: RouteMeteogramProps) {
  // Hover / frozen state: track hovered waypoint index + cloudCell
  // We also derive the date from crossSectionData[waypointIndex].date for the tooltip
  const [hoveredState, setHoveredState] = useState<{
    waypoint: RouteWaypoint;
    cloudCell: CloudCell;
  } | null>(null);

  const [frozenRect, setFrozenRect] = useState<{
    waypoint: RouteWaypoint;
    cloudCell: CloudCell;
  } | null>(null);

  // Derive date for tooltip from crossSectionData using waypoint index
  const getDateForWaypoint = useCallback(
    (waypoint: RouteWaypoint): Date => {
      const index = waypoints.indexOf(waypoint);
      if (index >= 0 && index < crossSectionData.length) {
        return crossSectionData[index].date;
      }
      return new Date(0);
    },
    [waypoints, crossSectionData],
  );

  // Compute drawing bounds
  const bounds = useMemo(() => {
    const xMax = width - margin.left - margin.right;
    const yMax = height - margin.top - margin.bottom;
    return { xMax, yMax };
  }, [width, height, margin]);

  const { xMax, yMax } = bounds;

  // Scales
  const scales = useRouteScales(waypoints, bounds, clampCloudCoverageAt50Pct);

  // Pressure levels from model config
  const pressureLevels = useMemo(
    () => MODEL_CONFIGS[model].hpaLevels,
    [model],
  );

  // windBarbPressureLevelStep from model config
  const windBarbPressureLevelStep = useMemo(
    () => MODEL_CONFIGS[model].windBarbPressureLevelStep,
    [model],
  );

  // windBarbPointStep: every other sample point
  const windBarbPointStep = 2;

  // maxStepDistance for contour lines: conservative based on waypoint count
  const maxStepDistance = useMemo(
    () => Math.min(6, Math.ceil(waypoints.length / 5) || 2),
    [waypoints.length],
  );

  // Handlers
  const handleHover = useCallback(
    (waypoint: RouteWaypoint | null, cloudCell: CloudCell | null) => {
      if (waypoint && cloudCell) {
        setHoveredState({ waypoint, cloudCell });
      } else {
        setHoveredState(null);
      }
    },
    [],
  );

  const handleFreezeChange = useCallback(
    (rect: { waypoint: RouteWaypoint; cloudCell: CloudCell } | null) => {
      setFrozenRect(rect);
    },
    [],
  );

  // Active display state (frozen takes priority)
  const activeState = frozenRect ?? hoveredState;

  // Tooltip position: clamp to SVG bounds
  const tooltipPos = useMemo(() => {
    if (!activeState) return null;
    const { waypoint, cloudCell } = activeState;
    const rawX = scales.distanceScale(waypoint.distanceNM) + margin.left + 10;
    const rawY = scales.mslScale(cloudCell.mslFtTop) + margin.top - 10;
    return {
      x: formatNumber(Math.min(rawX, xMax + margin.left - 210)),
      y: formatNumber(Math.min(rawY, yMax + margin.top - 160)),
    };
  }, [activeState, scales, margin, xMax, yMax]);

  return (
    <svg
      width={formatNumber(width)}
      height={formatNumber(height)}
      className="route-meteogram"
      onMouseLeave={() => {
        if (!frozenRect) {
          setHoveredState(null);
        }
      }}
    >
      {/* Sky blue background */}
      <rect
        className="route-meteogram-background"
        x={0}
        y={0}
        width={formatNumber(width)}
        height={formatNumber(height)}
        fill={background}
        rx={14}
      />

      <Group
        top={formatNumber(margin.top)}
        left={formatNumber(margin.left)}
        className="route-meteogram-content"
      >
        {/* Layer 1: Terrain Profile */}
        <TerrainProfile
          waypoints={waypoints}
          elevations={elevations}
          scales={scales}
          yMax={yMax}
        />

        {/* Layer 2: Cloud Columns + Wind Barbs */}
        <RouteCloudColumns
          crossSectionData={crossSectionData}
          waypoints={waypoints}
          scales={scales}
          pressureLevels={pressureLevels}
          highlightCeilingCoverage={highlightCeilingCoverage}
          clampCloudCoverageAt50Pct={clampCloudCoverageAt50Pct}
          showWindBarbs={showWindBarbs}
          windBarbPointStep={windBarbPointStep}
          windBarbPressureLevelStep={windBarbPressureLevelStep}
          frozenRect={frozenRect}
          onHover={handleHover}
          onFreezeChange={handleFreezeChange}
        />

        {/* Layer 3: Weather Lines (isotherms, isotachs, dew point depression, freezing level) */}
        <RouteWeatherLines
          crossSectionData={crossSectionData}
          waypoints={waypoints}
          scales={scales}
          showIsothermLines={showIsothermLines}
          showIsotachLines={showIsotachLines}
          showDewPointDepressionLines={showDewPointDepressionLines}
          maxStepDistance={maxStepDistance}
        />

        {/* Layer 4: Pressure Lines (conditional) */}
        {showPressureLines && (
          <RoutePressureLines
            crossSectionData={crossSectionData}
            waypoints={waypoints}
            scales={scales}
            pressureLevels={pressureLevels}
          />
        )}

        {/* Layer 5: Cruise Altitude Line */}
        <AltitudeLine
          cruiseAltitudeFt={cruiseAltitudeFt}
          waypoints={waypoints}
          crossSectionData={crossSectionData}
          scales={scales}
          showWindBarbs={showWindBarbs}
        />

        {/* Layer 6: Waypoint Markers */}
        <WaypointMarkers
          waypoints={waypoints}
          scales={scales}
          yMax={yMax}
          model={model}
        />
      </Group>

      {/* Axes rendered on top */}
      <g className="axes-group">
        {/* Y-axis: altitude in ft MSL */}
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

        {/* X-axis: distance in NM */}
        <AxisBottom
          axisClassName="distance-axis"
          left={formatNumber(margin.left)}
          top={formatNumber(yMax + margin.top)}
          scale={scales.distanceScale}
          numTicks={10}
          tickFormat={(value) => `${formatNumber(Number(value))}`}
          stroke={black}
          tickStroke={black}
          tickLabelProps={{
            fill: black,
            fontSize: 11,
            textAnchor: "middle",
          }}
        />

        {/* Axis unit labels */}
        <g className="axis-units">
          <text
            x={formatNumber(margin.left - 5)}
            y={formatNumber(yMax + margin.top + 30)}
            fontSize={11}
            fill={black}
            textAnchor="end"
          >
            ft MSL
          </text>
          <text
            x={formatNumber(margin.left + xMax / 2)}
            y={formatNumber(yMax + margin.top + 36)}
            fontSize={11}
            fill={black}
            textAnchor="middle"
          >
            NM
          </text>
        </g>
      </g>

      {/* Tooltip */}
      {activeState && tooltipPos && (
        <MeteogramTooltip
          date={getDateForWaypoint(activeState.waypoint)}
          cloudCell={activeState.cloudCell}
          x={tooltipPos.x}
          y={tooltipPos.y}
          useLocalTime={useLocalTime}
          frozen={frozenRect !== null}
        />
      )}
    </svg>
  );
});

export default RouteMeteogram;
