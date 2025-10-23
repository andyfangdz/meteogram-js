import React from "react";
import { Group } from "@visx/group";
import { CloudColumn, CloudCell } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import WindBarb from "./wind-barb";
import { MODEL_CONFIGS } from "../../config/weather";
import { WeatherModel } from "../../types/weather";

interface CloudColumnsProps {
  weatherData: CloudColumn[];
  scales: {
    dateScale: any;
    mslScale: any;
    cloudScale: any;
  };
  barWidth: number;
  pressureLevels: number[];
  highlightCeilingCoverage: boolean;
  clampCloudCoverageAt50Pct: boolean;
  showWindBarbs: boolean;
  model: WeatherModel;
  frozenRect: { date: Date; cloudCell: CloudCell } | null;
  onHover: (date: Date | null, cloudCell: CloudCell | null) => void;
  onFreezeChange: (rect: { date: Date; cloudCell: CloudCell } | null) => void;
}

const CloudColumns: React.FC<CloudColumnsProps> = ({
  weatherData,
  scales,
  barWidth,
  pressureLevels,
  highlightCeilingCoverage,
  clampCloudCoverageAt50Pct,
  showWindBarbs,
  model,
  frozenRect,
  onHover,
  onFreezeChange,
}) => {
  // Convert pressureLevels array to Set for O(1) lookups during filtering
  // This optimization is beneficial when filtering large datasets (many cloud cells per column)
  // Trade-off: Set creation has overhead, but pays off when pressureLevels.length > ~10
  // and when weatherData contains many columns with many cloud cells each
  const pressureLevelsSet = React.useMemo(() => new Set(pressureLevels), [pressureLevels]);

  // Memoize filtered weather data to avoid recomputing on every render
  const filteredWeatherData = React.useMemo(() => {
    return weatherData.map((d) => ({
      ...d,
      filteredClouds: d.cloud?.filter(
        (cloud) => cloud.hpa != null && pressureLevelsSet.has(cloud.hpa),
      ) || [],
    }));
  }, [weatherData, pressureLevelsSet]);

  // Stable event handlers using useCallback
  // IMPORTANT: These callbacks depend on onHover and onFreezeChange being stable.
  // The parent component MUST memoize these callbacks (e.g., with useCallback)
  // to prevent these handlers from being recreated on every render.
  // Currently verified: meteogram.tsx memoizes both callbacks correctly.
  const handleMouseEnter = React.useCallback((date: Date, cloud: CloudCell) => {
    if (!frozenRect) {
      onHover(date, cloud);
    }
  }, [frozenRect, onHover]);

  const handleMouseLeave = React.useCallback(() => {
    if (!frozenRect) {
      onHover(null, null);
    }
  }, [frozenRect, onHover]);

  const handleClick = React.useCallback((date: Date, cloud: CloudCell, event: React.MouseEvent) => {
    if ((event.nativeEvent as PointerEvent).pointerType === "mouse") {
      if (frozenRect) {
        onFreezeChange(null);
        onHover(date, cloud);
      } else {
        onFreezeChange({ date, cloudCell: cloud });
        onHover(date, cloud);
      }
    }
  }, [frozenRect, onFreezeChange, onHover]);

  return (
    <>
      {filteredWeatherData.map((d) => {
        const { filteredClouds } = d;

        return (
          <Group
            key={`date-group-${d.date}`}
            left={formatNumber(scales.dateScale(d.date))}
            className="cloud-column"
          >
            {filteredClouds?.map((cloud) => {
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
                  width={formatNumber(barWidth)}
                  height={formatNumber(
                    scales.mslScale(cloud.mslFtBottom) -
                      scales.mslScale(cloud.mslFtTop),
                  )}
                  fill={fillColor}
                  stroke="transparent"
                  strokeWidth={0}
                  style={{ cursor: "default" }}
                  onMouseEnter={() => handleMouseEnter(d.date, cloud)}
                  onMouseLeave={handleMouseLeave}
                  onClick={(event) => handleClick(d.date, cloud, event)}
                />
              );
            })}
          </Group>
        );
      })}

      {/* Wind Barbs */}
      {showWindBarbs &&
        model &&
        MODEL_CONFIGS[model] &&
        weatherData
          .filter((_, index) => index % MODEL_CONFIGS[model].windBarbStep === 0)
          .map((d) =>
            d.cloud
              ?.filter(
                (cloud, levelIndex) =>
                  pressureLevelsSet.has(cloud.hpa) &&
                  levelIndex %
                    MODEL_CONFIGS[model].windBarbPressureLevelStep ===
                    0 &&
                  cloud.windSpeed != null &&
                  cloud.windDirection != null &&
                  cloud.geopotentialFt != null,
              )
              .map((cloud) => (
                <g
                  key={`wind-barb-${d.date}-${cloud.hpa}`}
                  className={`wind-barb-group wind-barb-${cloud.hpa}`}
                >
                  <WindBarb
                    x={formatNumber(scales.dateScale(d.date) + barWidth / 2)}
                    y={formatNumber(scales.mslScale(cloud.geopotentialFt))}
                    speed={cloud.windSpeed}
                    direction={cloud.windDirection}
                  />
                </g>
              )),
          )}
    </>
  );
};

export default React.memo(CloudColumns);
