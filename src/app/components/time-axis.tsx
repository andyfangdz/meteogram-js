"use client";

import dynamic from "next/dynamic";
import type { AxisBottom as AxisBottomType, AxisScale } from "@visx/axis";
import { timeFormat, utcFormat } from "@visx/vendor/d3-time-format";
import { useSyncExternalStore, useCallback, useMemo } from "react";

const AxisBottom = dynamic<React.ComponentProps<typeof AxisBottomType>>(
  () => import("@visx/axis").then((mod) => mod.AxisBottom),
  {
    ssr: false,
    loading: () => null, // Render nothing while loading
  },
);

function subscribe() {
  return () => {};
}

function getSnapshot() {
  return true;
}

function getServerSnapshot() {
  return false;
}

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
  const mounted = useSyncExternalStore(subscribe, getSnapshot, getServerSnapshot);

  // Memoize tickFormat to avoid recreating on every render
  const tickFormat = useCallback((value: Date | number) => {
    if (!mounted) {
      return ""; // Don't show any ticks until mounted
    }
    if (value instanceof Date) {
      return useLocalTime
        ? timeFormat("%d%H")(value)
        : utcFormat("%d%HZ")(value);
    }
    const date = new Date(Number(value));
    return useLocalTime
      ? timeFormat("%d%H")(date)
      : utcFormat("%d%HZ")(date);
  }, [mounted, useLocalTime]);

  // Memoize tickLabelProps to avoid recreating on every render
  const tickLabelProps = useMemo(() => ({
    fill: stroke,
    fontSize: 11,
    textAnchor: "middle" as const,
  }), [stroke]);

  // Always render the component, but conditionally render the tick labels
  return (
    <AxisBottom
      left={left}
      top={top}
      scale={scale}
      tickFormat={tickFormat}
      stroke={stroke}
      tickStroke={tickStroke}
      tickLabelProps={tickLabelProps}
    />
  );
}
