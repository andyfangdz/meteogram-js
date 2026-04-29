import React from "react";
import { Group } from "@visx/group";
import { CloudColumn, CloudCell } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";
import {
  getInstabilityColor,
  getBuoyancyColor,
} from "../../utils/lapseRate";
import type { ParcelProfile } from "../../utils/condensation";
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
  showStabilityTint: boolean;
  showParcelBuoyancy: boolean;
  parcelProfiles: ParcelProfile[];
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
  showStabilityTint,
  showParcelBuoyancy,
  parcelProfiles,
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

  // Memoize filtered weather data to avoid recomputing on every render.
  // We also carry a parallel filteredParcelTemps array — parcelProfiles is
  // indexed by the original (unfiltered) cloud index, so once we filter
  // cells out we'd lose alignment without keeping them side-by-side.
  const filteredWeatherData = React.useMemo(() => {
    return weatherData.map((d, columnIdx) => {
      const profile = parcelProfiles[columnIdx];
      const filteredClouds: CloudCell[] = [];
      const filteredParcelTemps: (number | null)[] = [];
      d.cloud?.forEach((cloud, origIdx) => {
        if (cloud.hpa != null && pressureLevelsSet.has(cloud.hpa)) {
          filteredClouds.push(cloud);
          filteredParcelTemps.push(profile?.parcelTempC[origIdx] ?? null);
        }
      });
      return { ...d, filteredClouds, filteredParcelTemps };
    });
  }, [weatherData, pressureLevelsSet, parcelProfiles]);

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

  // Tint extents from filtered neighbors. mslFtTop/mslFtBottom on the cell
  // come from the unfiltered list, so when a level is dropped from the
  // pressure-level set the surrounding cells leave a vertical gap exactly
  // the size of the missing layer. Using midpoints between filtered
  // neighbors makes adjacent tints tile without gaps.
  const getTintBounds = React.useCallback(
    (filteredClouds: CloudCell[], idx: number) => {
      const cloud = filteredClouds[idx];
      const prev = idx > 0 ? filteredClouds[idx - 1] : null;
      const next =
        idx < filteredClouds.length - 1 ? filteredClouds[idx + 1] : null;
      return {
        tintBottom: prev
          ? (prev.mslFt + cloud.mslFt) / 2
          : cloud.mslFtBottom,
        tintTop: next
          ? (cloud.mslFt + next.mslFt) / 2
          : cloud.mslFtTop,
      };
    },
    [],
  );

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
        const { filteredClouds, filteredParcelTemps } = d;

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
            {showStabilityTint &&
              filteredClouds.map((cloud, idx) => {
                if (cloud.instabilityKPerKm == null) return null;
                const fill = getInstabilityColor(cloud.instabilityKPerKm);
                if (!fill) return null;
                const { tintTop, tintBottom } = getTintBounds(
                  filteredClouds,
                  idx,
                );
                return (
                  <rect
                    className={`stability-tint stability-${
                      cloud.instabilityKPerKm > 0 ? "unstable" : "stable"
                    }`}
                    key={`stability-${cloud.hpa}`}
                    x={formatNumber(0)}
                    y={formatNumber(scales.mslScale(tintTop))}
                    width={formatNumber(barWidth)}
                    height={formatNumber(
                      scales.mslScale(tintBottom) - scales.mslScale(tintTop),
                    )}
                    fill={fill}
                    pointerEvents="none"
                  />
                );
              })}
            {showParcelBuoyancy &&
              filteredClouds.map((cloud, idx) => {
                const parcelT = filteredParcelTemps[idx];
                if (parcelT == null) return null;
                const buoyancy = parcelT - cloud.temperature;
                const fill = getBuoyancyColor(buoyancy);
                if (!fill) return null;
                const { tintTop, tintBottom } = getTintBounds(
                  filteredClouds,
                  idx,
                );
                return (
                  <rect
                    className={`parcel-tint parcel-${
                      buoyancy > 0 ? "cape" : "cin"
                    }`}
                    key={`parcel-${cloud.hpa}`}
                    x={formatNumber(0)}
                    y={formatNumber(scales.mslScale(tintTop))}
                    width={formatNumber(barWidth)}
                    height={formatNumber(
                      scales.mslScale(tintBottom) - scales.mslScale(tintTop),
                    )}
                    fill={fill}
                    pointerEvents="none"
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
