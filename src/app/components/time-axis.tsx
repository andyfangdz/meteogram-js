"use client";

import { AxisBottom, AxisScale } from "@visx/axis";
import { timeFormat, utcFormat } from "@visx/vendor/d3-time-format";
import { scaleTime } from "@visx/scale";

interface TimeAxisProps {
  left: number;
  top: number;
  scale: AxisScale;
  useLocalTime: boolean;
  stroke: string;
  tickStroke: string;
}

export default function TimeAxis({
  left,
  top,
  scale,
  useLocalTime,
  stroke,
  tickStroke,
}: TimeAxisProps) {
  return (
    <AxisBottom
      left={left}
      top={top}
      scale={scale}
      tickFormat={(value) => {
        if (value instanceof Date) {
          return useLocalTime
            ? timeFormat("%d%H")(value)
            : utcFormat("%d%HZ")(value);
        }
        const date = new Date(Number(value));
        return useLocalTime
          ? timeFormat("%d%H")(date)
          : utcFormat("%d%HZ")(date);
      }}
      stroke={stroke}
      tickStroke={tickStroke}
      tickLabelProps={{
        fill: stroke,
        fontSize: 11,
        textAnchor: "middle",
      }}
    />
  );
}
