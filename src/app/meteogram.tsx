"use client";

import React from "react";
import { Group } from "@visx/group";
import { AxisBottom, AxisLeft } from "@visx/axis";
import { scaleBand, scaleLinear } from "@visx/scale";
import { utcFormat, timeFormat } from "@visx/vendor/d3-time-format";

import { CloudData } from "./meteo-vars";
import { Spinner } from "@nextui-org/spinner";
import LoadingSkeleton from "./loading-skeleton";

export type MeteogramProps = {
  width: number;
  height: number;
  margin?: { top: number; right: number; bottom: number; left: number };
  useLocalTime?: boolean;
  weatherData: CloudData[];
};
const black = "#000000";
const background = "#87CEEB";

const defaultMargin = { top: 40, right: 20, bottom: 40, left: 60 };

export default function Meteogram({
  width,
  height,
  weatherData,
  margin = defaultMargin,
  useLocalTime = false,
}: MeteogramProps) {


  if (weatherData.length === 0) {
    return <LoadingSkeleton />;
  }

  // bounds
  const xMax = width - margin.left - margin.right;
  const yMax = height - margin.top - margin.bottom;

  // update scale output dimensions
  // scales
  const dateScale = scaleBand<Date>({
    domain: weatherData.map((d) => d.date),
    // padding: 0.2,
  }).range([0, xMax]).align(0);

  const mslScale = scaleLinear<number>({
    domain: [0, 20_000],
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
