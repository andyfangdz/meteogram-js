"use client";

import React, { useState, useEffect } from "react";
import { Group } from "@visx/group";
import { BarGroup } from "@visx/shape";
import { AxisBottom, AxisLeft } from "@visx/axis";
import cityTemperature, {
  CityTemperature,
} from "@visx/mock-data/lib/mocks/cityTemperature";
import { scaleBand, scaleLinear, scaleOrdinal } from "@visx/scale";
import { timeParse, utcFormat, timeFormat } from "@visx/vendor/d3-time-format";

import fetchWeatherData, { CloudData } from "./meteo-vars";

export type BarGroupProps = {
  width: number;
  height: number;
  margin?: { top: number; right: number; bottom: number; left: number };
  events?: boolean;
  useLocalTime?: boolean;
};

type CityName = "New York" | "San Francisco" | "Austin";

const blue = "#aeeef8";
export const green = "#e5fd3d";
const black = "#000000";
const purple = "#9caff6";
export const background = "#87CEEB";
const xPadding = 60;
const yPadding = 20;

const data = cityTemperature.slice(0, 8);
const keys = Object.keys(data[0]).filter((d) => d !== "date") as CityName[];
const defaultMargin = { top: 40, right: 60, bottom: 40, left: 60 };

const parseDate = timeParse("%Y-%m-%d");

// accessors
const getDate = (d: CityTemperature) => d.date;

const cityScale = scaleBand<string>({
  domain: keys,
  padding: 0.1,
});
const colorScale = scaleOrdinal<string, string>({
  domain: keys,
  range: [blue, green, purple],
});

export default function Meteogram({
  width,
  height,
  events = false,
  margin = defaultMargin,
  useLocalTime = false,
}: BarGroupProps) {
  let [weatherData, setWeatherData] = useState<CloudData[]>([]);
  useEffect(() => {
    fetchWeatherData().then((data) => setWeatherData(data));
  }, []);

  if (weatherData.length === 0) {
    return null;
  }

  // bounds
  const xMax = width - margin.left - margin.right;
  const yMax = height - margin.top - margin.bottom;

  // update scale output dimensions
  // scales
  const dateScale = scaleBand<Date>({
    domain: weatherData.map((d) => d.date),
    // padding: 0.2,
  }).rangeRound([0, xMax]);

  const mslScale = scaleLinear<number>({
    domain: [0, 15_000],
  }).range([yMax, 0]);

  const cloudScale = scaleLinear<number>({
    domain: [0, 50],
  }).range([0, 1]);

  return weatherData === null ? null : (
    <svg width={width} height={height}>
      <rect
        x={0}
        y={0}
        width={width}
        height={height}
        fill={background}
        rx={14}
      />
      <Group top={margin.top} left={margin.left}>
        {weatherData.map((d) => (
          <Group key={`date-group-${d.date}`} left={dateScale(d.date)}>
            {d.cloud.map((cloud, i) => (
              <rect
                key={`cloud-group-${d.date}-${cloud.hpa}`}
                x={0}
                y={mslScale(cloud.mslFtTop)}
                width={dateScale.bandwidth()}
                height={mslScale(cloud.mslFtBottom) - mslScale(cloud.mslFtTop)}
                fill={`rgba(255, 255, 255, ${cloudScale(cloud.cloudCoverage)})`}
                stroke={cloud.cloudCoverage > 50 ? black : "transparent"}
                strokeWidth={cloud.cloudCoverage > 50 ? 1 : 0}
              />
            ))}
          </Group>
        ))}
      </Group>
      <AxisBottom
        left={margin.left}
        top={yMax + margin.top}
        tickFormat={useLocalTime ? timeFormat("%d%H") : utcFormat("%d%HZ")}
        scale={dateScale}
        stroke={black}
        tickStroke={black}
        // hideAxisLine
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
        // hideAxisLine
        tickLabelProps={{
          fill: black,
          fontSize: 11,
          textAnchor: "end",
        }}
      />
    </svg>
  );
}
