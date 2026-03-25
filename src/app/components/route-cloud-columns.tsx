import React from "react";
import { Group } from "@visx/group";
import { CloudColumn, CloudCell, RouteWaypoint } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import WindBarb from "./wind-barb";
import { RouteScales } from "../../hooks/useRouteScales";

interface RouteCloudColumnsProps {
  crossSectionData: CloudColumn[];
  waypoints: RouteWaypoint[];
  scales: RouteScales;
  pressureLevels: number[];
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showWindBarbs: boolean;
  windBarbPointStep: number;
  windBarbPressureLevelStep: number;
  frozenRect: { waypoint: RouteWaypoint; cloudCell: CloudCell } | null;
  onHover: (waypoint: RouteWaypoint | null, cloudCell: CloudCell | null) => void;
  onFreezeChange: (
    rect: { waypoint: RouteWaypoint; cloudCell: CloudCell } | null,
  ) => void;
}

const RouteCloudColumns: React.FC<RouteCloudColumnsProps> = ({
  crossSectionData,
  waypoints,
  scales,
  pressureLevels,
  highlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  showWindBarbs,
  windBarbPointStep,
  windBarbPressureLevelStep,
  frozenRect,
  onHover,
  onFreezeChange,
}) => {
  const pressureLevelsSet = React.useMemo(
    () => new Set(pressureLevels),
    [pressureLevels],
  );

  // Pair each CloudColumn with its corresponding RouteWaypoint
  // crossSectionData[i] corresponds to waypoints[i]
  const pairedData = React.useMemo(() => {
    return crossSectionData.map((column, index) => {
      const wp = waypoints[index];
      const prevWp = index > 0 ? waypoints[index - 1] : null;
      const nextWp = index < waypoints.length - 1 ? waypoints[index + 1] : null;

      // Compute bar width from adjacent waypoint distances
      let barWidthNM: number;
      if (prevWp && nextWp) {
        barWidthNM = (nextWp.distanceNM - prevWp.distanceNM) / 2;
      } else if (prevWp) {
        barWidthNM = wp.distanceNM - prevWp.distanceNM;
      } else if (nextWp) {
        barWidthNM = nextWp.distanceNM - wp.distanceNM;
      } else {
        barWidthNM = 1;
      }

      const barWidthPx = Math.abs(
        scales.distanceScale(barWidthNM) - scales.distanceScale(0),
      );

      // Center the bar on the waypoint position
      const xCenter = scales.distanceScale(wp.distanceNM);
      const xLeft = xCenter - barWidthPx / 2;

      const filteredClouds =
        column.cloud?.filter(
          (cloud) => cloud.hpa != null && pressureLevelsSet.has(cloud.hpa),
        ) ?? [];

      return { column, wp, index, xLeft, xCenter, barWidthPx, filteredClouds };
    });
  }, [crossSectionData, waypoints, scales, pressureLevelsSet]);

  const handleMouseEnter = React.useCallback(
    (wp: RouteWaypoint, cloud: CloudCell) => {
      if (!frozenRect) {
        onHover(wp, cloud);
      }
    },
    [frozenRect, onHover],
  );

  const handleMouseLeave = React.useCallback(() => {
    if (!frozenRect) {
      onHover(null, null);
    }
  }, [frozenRect, onHover]);

  const handleClick = React.useCallback(
    (wp: RouteWaypoint, cloud: CloudCell, event: React.MouseEvent) => {
      if ((event.nativeEvent as PointerEvent).pointerType === "mouse") {
        if (frozenRect) {
          onFreezeChange(null);
          onHover(wp, cloud);
        } else {
          onFreezeChange({ waypoint: wp, cloudCell: cloud });
          onHover(wp, cloud);
        }
      }
    },
    [frozenRect, onFreezeChange, onHover],
  );

  return (
    <>
      {pairedData.map(({ column, wp, xLeft, barWidthPx, filteredClouds }) => (
        <Group
          key={`wp-group-${wp.distanceNM}-${wp.name}`}
          left={formatNumber(xLeft)}
          className="cloud-column"
        >
          {filteredClouds.map((cloud) => {
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
                width={formatNumber(barWidthPx)}
                height={formatNumber(
                  scales.mslScale(cloud.mslFtBottom) -
                    scales.mslScale(cloud.mslFtTop),
                )}
                fill={fillColor}
                stroke="transparent"
                strokeWidth={0}
                style={{ cursor: "default" }}
                onMouseEnter={() => handleMouseEnter(wp, cloud)}
                onMouseLeave={handleMouseLeave}
                onClick={(event) => handleClick(wp, cloud, event)}
              />
            );
          })}
        </Group>
      ))}

      {/* Wind Barbs */}
      {showWindBarbs &&
        pairedData
          .filter(({ index }) => index % windBarbPointStep === 0)
          .map(({ column, wp, xCenter }) =>
            column.cloud
              ?.filter(
                (cloud, levelIndex) =>
                  pressureLevelsSet.has(cloud.hpa) &&
                  levelIndex % windBarbPressureLevelStep === 0 &&
                  cloud.windSpeed != null &&
                  cloud.windDirection != null &&
                  cloud.geopotentialFt != null,
              )
              .map((cloud) => (
                <g
                  key={`wind-barb-${wp.distanceNM}-${wp.name}-${cloud.hpa}`}
                  className={`wind-barb-group wind-barb-${cloud.hpa}`}
                >
                  <WindBarb
                    x={formatNumber(xCenter)}
                    y={formatNumber(scales.mslScale(cloud.geopotentialFt!))}
                    speed={cloud.windSpeed!}
                    direction={cloud.windDirection!}
                  />
                </g>
              )),
          )}
    </>
  );
};

export default React.memo(RouteCloudColumns);
