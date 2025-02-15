import React from "react";
import { CloudColumn, WeatherModel } from "../../types/weather";
import {
  formatNumber,
  getTemperatureColor,
  findFreezingLevels,
  findIsothermPoints,
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

interface WeatherLinesProps {
  weatherData: CloudColumn[];
  scales: {
    dateScale: any;
    mslScale: any;
  };
  showIsothermLines: boolean;
  model: WeatherModel;
}

const WeatherLines: React.FC<WeatherLinesProps> = ({
  weatherData,
  scales,
  showIsothermLines,
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

  return (
    <>
      {/* Freezing Levels */}
      {freezingPoints.map(({ points }, lineIndex: number) => {
        if (!points.length) return null;

        const pathD = points.reduce((path: string, point: Point, i: number) => {
          const x = formatNumber(scales.dateScale(weatherData[point.x].date));
          const y = formatNumber(scales.mslScale(point.y));
          if (i === 0) return `M ${x} ${y}`;
          return `${path} L ${x} ${y}`;
        }, "");

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
        isothermPoints.map(
          ({ temp, points }: IsothermLine, lineIndex: number) => {
            if (!points.length) return null;

            const pathD = points.reduce(
              (path: string, point: Point, i: number) => {
                const x = formatNumber(
                  scales.dateScale(weatherData[point.x].date),
                );
                const y = formatNumber(scales.mslScale(point.y));
                if (i === 0) return `M ${x} ${y}`;
                return `${path} L ${x} ${y}`;
              },
              "",
            );

            // Only render if we have valid points and a valid path
            if (!pathD) return null;

            return (
              <g
                key={`isotherm-${temp}-${formatNumber(points[0].y)}-${lineIndex}`}
                className={`isotherm-group isotherm-${temp}`}
                pointerEvents="none"
              >
                <path
                  className="isotherm-line"
                  d={pathD}
                  stroke={getTemperatureColor(temp)}
                  strokeWidth={1}
                  strokeDasharray="4,4"
                  opacity={0.7}
                  fill="none"
                />
                <text
                  className="isotherm-label"
                  x={formatNumber(
                    scales.dateScale(weatherData[points[0].x].date),
                  )}
                  y={formatNumber(scales.mslScale(points[0].y))}
                  dx="-2.5em"
                  dy="0.3em"
                  fontSize="10"
                  fill={getTemperatureColor(temp)}
                  pointerEvents="none"
                >
                  {`${temp}°C`}
                </text>
              </g>
            );
          },
        )}
    </>
  );
};

export default React.memo(WeatherLines);
