import { useMemo } from "react";
import { scaleLinear } from "@visx/scale";
import type { RouteWaypoint } from "../types/weather";

interface Bounds {
  xMax: number;
  yMax: number;
}

export interface RouteScales {
  distanceScale: ReturnType<typeof scaleLinear<number>>;
  mslScale: ReturnType<typeof scaleLinear<number>>;
  cloudScale: ReturnType<typeof scaleLinear<number>>;
}

export const useRouteScales = (
  waypoints: RouteWaypoint[],
  bounds: Bounds,
  clampCloudCoverageAt50Pct: boolean,
): RouteScales => {
  return useMemo(() => {
    const maxDistance = waypoints.length > 0
      ? waypoints[waypoints.length - 1].distanceNM
      : 100;
    return {
      distanceScale: scaleLinear<number>({
        domain: [0, maxDistance],
        range: [0, bounds.xMax],
      }),
      mslScale: scaleLinear<number>({
        domain: [0, 20_000],
        range: [bounds.yMax, 0],
      }),
      cloudScale: scaleLinear<number>({
        domain: [0, clampCloudCoverageAt50Pct ? 50 : 75],
        range: [0, 1],
        clamp: true,
      }),
    };
  }, [bounds, waypoints, clampCloudCoverageAt50Pct]);
};
