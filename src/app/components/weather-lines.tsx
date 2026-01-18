import React from "react";
import { CloudColumn, WeatherModel } from "../../types/weather";
import {
  formatNumber,
  getTemperatureColor,
  getWindSpeedColor,
  findFreezingLevels,
  findIsothermPoints,
  findIsotachPoints,
  findDewPointDepressionPoints,
} from "../../utils/meteogram";
import { MODEL_CONFIGS } from "../../config/weather";

interface Point {
  x: number;
  y: number;
}

interface FreezingLine {
  points: Point[];
}

interface IsothermLine {
  temp: number;
  points: Point[];
}

interface IsotachLine {
  speedKnots: number;
  points: Point[];
}

interface DewPointDepressionLine {
  spread: number;
  points: Point[];
}

// Color scheme for dew point depression: cyan (moist) -> yellow -> orange (dry)
const DEW_POINT_DEPRESSION_COLORS: Record<number, string> = {
  0: "#FF00FF", // Magenta - saturated
  1: "#00FFFF", // Cyan - nearly saturated
  3: "#00CED1", // Dark cyan - near saturation
  5: "#FFD700", // Yellow - moderate
  10: "#FF8C00", // Orange - dry
};

const getDewPointDepressionColor = (spread: number): string => {
  return DEW_POINT_DEPRESSION_COLORS[spread] || "#888888";
};

interface WeatherLinesProps {
  weatherData: CloudColumn[];
  scales: {
    dateScale: any;
    mslScale: any;
  };
  showIsothermLines: boolean;
  showIsotachLines: boolean;
  showDewPointDepressionLines: boolean;
  model: WeatherModel;
}

const WeatherLines: React.FC<WeatherLinesProps> = ({
  weatherData,
  scales,
  showIsothermLines,
  showIsotachLines,
  showDewPointDepressionLines,
  model,
}) => {
  // Convert the utility function results into the format we need
  const freezingPoints = React.useMemo(() => {
    return findFreezingLevels(weatherData);
  }, [weatherData]);

  const isothermPoints = React.useMemo(() => {
    if (!showIsothermLines) return [];
    return findIsothermPoints(
      weatherData,
      2,
      500,
      MODEL_CONFIGS[model].maxIsothermStepDistance,
    );
  }, [weatherData, showIsothermLines, model]);

  const isotachPoints = React.useMemo(() => {
    if (!showIsotachLines) return [];
    return findIsotachPoints(
      weatherData,
      10,
      500,
      MODEL_CONFIGS[model].maxIsothermStepDistance,
    );
  }, [weatherData, showIsotachLines, model]);

  const dewPointDepressionPoints = React.useMemo(() => {
    if (!showDewPointDepressionLines) return [];
    return findDewPointDepressionPoints(weatherData, [0, 1, 3, 5, 10]);
  }, [weatherData, showDewPointDepressionLines]);

  // Memoize freezing level paths to avoid rebuilding on every render
  const freezingPaths = React.useMemo(() => {
    return freezingPoints.map(({ points }) => {
      if (!points.length) return null;
      return points.reduce((path: string, point: Point, i: number) => {
        const x = formatNumber(scales.dateScale(weatherData[point.x].date));
        const y = formatNumber(scales.mslScale(point.y));
        if (i === 0) return `M ${x} ${y}`;
        return `${path} L ${x} ${y}`;
      }, "");
    });
  }, [freezingPoints, scales, weatherData]);

  // Memoize isotherm paths with colors to avoid rebuilding on every render
  const isothermPaths = React.useMemo(() => {
    return isothermPoints.map(({ temp, points }) => {
      if (!points.length) return null;
      const pathD = points.reduce((path: string, point: Point, i: number) => {
        const x = formatNumber(scales.dateScale(weatherData[point.x].date));
        const y = formatNumber(scales.mslScale(point.y));
        if (i === 0) return `M ${x} ${y}`;
        return `${path} L ${x} ${y}`;
      }, "");
      return {
        temp,
        pathD,
        color: getTemperatureColor(temp),
        firstPoint: points[0],
      };
    });
  }, [isothermPoints, scales, weatherData]);

  // Memoize isotach paths with colors to avoid rebuilding on every render
  const isotachPaths = React.useMemo(() => {
    return isotachPoints.map(({ speedKnots, points }) => {
      if (!points.length) return null;
      const pathD = points.reduce((path: string, point: Point, i: number) => {
        const x = formatNumber(scales.dateScale(weatherData[point.x].date));
        const y = formatNumber(scales.mslScale(point.y));
        if (i === 0) return `M ${x} ${y}`;
        return `${path} L ${x} ${y}`;
      }, "");
      return {
        speedKnots,
        pathD,
        color: getWindSpeedColor(speedKnots),
        firstPoint: points[0],
      };
    });
  }, [isotachPoints, scales, weatherData]);

  // Memoize dew point depression paths with colors
  const dewPointDepressionPaths = React.useMemo(() => {
    return dewPointDepressionPoints.map(({ spread, points }) => {
      if (!points.length) return null;
      const pathD = points.reduce((path: string, point: Point, i: number) => {
        const x = formatNumber(scales.dateScale(weatherData[point.x].date));
        const y = formatNumber(scales.mslScale(point.y));
        if (i === 0) return `M ${x} ${y}`;
        return `${path} L ${x} ${y}`;
      }, "");
      return {
        spread,
        pathD,
        color: getDewPointDepressionColor(spread),
        firstPoint: points[0],
      };
    });
  }, [dewPointDepressionPoints, scales, weatherData]);

  return (
    <>
      {/* Freezing Levels */}
      {freezingPaths.map((pathD, lineIndex: number) => {
        if (!pathD) return null;

        return (
          <path
            className={`freezing-level freezing-level-${lineIndex + 1}`}
            key={`freezing-level-${lineIndex}`}
            d={pathD}
            stroke="#0066cc"
            strokeWidth={2}
            strokeDasharray="4,4"
            fill="none"
            pointerEvents="none"
          />
        );
      })}

      {/* Isotherm Lines */}
      {showIsothermLines &&
        isothermPaths.map((data, lineIndex: number) => {
          if (!data || !data.pathD) return null;

          const { temp, pathD, color, firstPoint } = data;

          return (
            <g
              key={`isotherm-${temp}-${formatNumber(firstPoint.y)}-${lineIndex}`}
              className={`isotherm-group isotherm-${temp}`}
              pointerEvents="none"
            >
              <path
                className="isotherm-line"
                d={pathD}
                stroke={color}
                strokeWidth={1}
                strokeDasharray="4,4"
                opacity={0.7}
                fill="none"
              />
              <text
                className="isotherm-label"
                x={formatNumber(
                  scales.dateScale(weatherData[firstPoint.x].date),
                )}
                y={formatNumber(scales.mslScale(firstPoint.y))}
                dx="-2.5em"
                dy="0.3em"
                fontSize="10"
                fill={color}
                pointerEvents="none"
              >
                {`${temp}°C`}
              </text>
            </g>
          );
        })}

      {/* Isotach Lines */}
      {showIsotachLines &&
        isotachPaths.map((data, lineIndex: number) => {
          if (!data || !data.pathD) return null;

          const { speedKnots, pathD, color, firstPoint } = data;

          return (
            <g
              key={`isotach-${speedKnots}-${formatNumber(firstPoint.y)}-${lineIndex}`}
              className={`isotach-group isotach-${speedKnots}`}
              pointerEvents="none"
            >
              <path
                className="isotach-line"
                d={pathD}
                stroke={color}
                strokeWidth={2}
                strokeDasharray="4,2"
                opacity={1}
                fill="none"
              />
              <text
                className="isotach-label"
                x={formatNumber(
                  scales.dateScale(weatherData[firstPoint.x].date),
                )}
                y={formatNumber(scales.mslScale(firstPoint.y))}
                dx="-2.5em"
                dy="0.3em"
                fontSize="12"
                fontWeight="bold"
                fill={color}
                stroke="white"
                strokeWidth="3"
                paintOrder="stroke"
                pointerEvents="none"
              >
                {`${speedKnots.toFixed(0)}kt`}
              </text>
            </g>
          );
        })}

      {/* Dew Point Depression Lines */}
      {showDewPointDepressionLines &&
        dewPointDepressionPaths.map((data, lineIndex: number) => {
          if (!data || !data.pathD) return null;

          const { spread, pathD, color, firstPoint } = data;

          return (
            <g
              key={`dew-point-depression-${spread}-${formatNumber(firstPoint.y)}-${lineIndex}`}
              className={`dew-point-depression-group dew-point-depression-${spread}`}
              pointerEvents="none"
            >
              <path
                className="dew-point-depression-line"
                d={pathD}
                stroke={color}
                strokeWidth={1.5}
                strokeDasharray="2,3"
                strokeLinecap="round"
                opacity={0.8}
                fill="none"
              />
              <text
                className="dew-point-depression-label"
                x={formatNumber(
                  scales.dateScale(weatherData[firstPoint.x].date),
                )}
                y={formatNumber(scales.mslScale(firstPoint.y))}
                dx="-2.5em"
                dy="0.3em"
                fontSize="10"
                fill={color}
                stroke="white"
                strokeWidth="2"
                paintOrder="stroke"
                pointerEvents="none"
              >
                {`Δ${spread}°`}
              </text>
            </g>
          );
        })}
    </>
  );
};

export default React.memo(WeatherLines);
