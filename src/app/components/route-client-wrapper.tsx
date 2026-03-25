"use client";

import { useMemo } from "react";
import { useRouter } from "next/navigation";
import { ParentSize } from "@visx/responsive";
import { HeroUIProvider } from "@heroui/react";
import {
  CloudColumn,
  RouteWaypoint,
  WeatherModel,
  VisualizationPreferences,
} from "../../types/weather";
import {
  PreferencesProvider,
  usePreferences,
} from "@/context/PreferencesContext";
import {
  serializeRouteParams,
  serializeVisualizationPreferences,
} from "@/utils/params";
import RouteHeader, { RouteHeaderUpdate } from "./route-header";
import RouteMeteogram from "./route-meteogram";

interface RouteClientWrapperProps {
  initialWaypointString: string;
  initialModel: WeatherModel;
  initialCrossSectionDataStr: string;
  initialWaypoints: RouteWaypoint[];
  initialElevations: Array<number | null>;
  initialRoutePoints: Array<{ estimatedTimeOver: string; bearingDeg: number }>;
  initialCruiseAltitudeFt: number;
  initialTasKnots: number;
  initialDepartureTime: string;
  initialResolutionNM: number;
  initialPreferences: VisualizationPreferences;
  cookieReadSuccess?: boolean;
}

function RouteClientWrapperInternal({
  initialWaypointString,
  initialModel,
  initialCrossSectionDataStr,
  initialWaypoints,
  initialElevations,
  initialCruiseAltitudeFt,
  initialTasKnots,
  initialDepartureTime,
  initialResolutionNM,
}: Omit<RouteClientWrapperProps, "initialPreferences" | "cookieReadSuccess">) {
  const router = useRouter();
  const { preferences } = usePreferences();

  const crossSectionData = useMemo<CloudColumn[]>(() => {
    try {
      return JSON.parse(initialCrossSectionDataStr).map((col: CloudColumn & { date: string }) => ({
        ...col,
        date: new Date(col.date),
      }));
    } catch (e) {
      console.error("Failed to parse cross-section data", e);
      return [];
    }
  }, [initialCrossSectionDataStr]);

  const handleUpdate = (update: RouteHeaderUpdate) => {
    const routeParams = serializeRouteParams({
      cruiseAltitudeFt: update.cruiseAltitudeFt,
      tasKnots: update.tasKnots,
      departureTime: new Date(update.departureTime),
      resolutionNM: update.resolutionNM,
    });

    const prefParams = serializeVisualizationPreferences(preferences);

    // Merge both param sets
    const allParams = new URLSearchParams();
    routeParams.forEach((value, key) => allParams.set(key, value));
    prefParams.forEach((value, key) => allParams.set(key, value));

    const encodedWaypoints = encodeURIComponent(update.waypoints);
    router.push(`/route/${encodedWaypoints}/${update.model}?${allParams.toString()}`);
  };

  return (
    <div className="min-h-screen flex flex-col">
      <RouteHeader
        waypoints={initialWaypointString}
        model={initialModel}
        cruiseAltitudeFt={initialCruiseAltitudeFt}
        tasKnots={initialTasKnots}
        departureTime={initialDepartureTime}
        resolutionNM={initialResolutionNM}
        onUpdate={handleUpdate}
        isLoading={false}
      />
      <div className="flex-1 p-2">
        <ParentSize>
          {({ width }) => (
            <RouteMeteogram
              crossSectionData={crossSectionData}
              waypoints={initialWaypoints}
              elevations={initialElevations}
              cruiseAltitudeFt={initialCruiseAltitudeFt}
              width={Math.max(width, 400)}
              height={Math.max(600, Math.round(width * 0.5))}
              model={initialModel}
              useLocalTime={preferences.useLocalTime}
              highlightCeilingCoverage={preferences.highlightCeilingCoverage}
              clampCloudCoverageAt50Pct={preferences.clampCloudCoverageAt50Pct}
              showPressureLines={preferences.showPressureLines}
              showWindBarbs={preferences.showWindBarbs}
              showIsothermLines={preferences.showIsothermLines}
              showIsotachLines={preferences.showIsotachLines}
              showDewPointDepressionLines={preferences.showDewPointDepressionLines}
            />
          )}
        </ParentSize>
      </div>
    </div>
  );
}

export default function RouteClientWrapper(props: RouteClientWrapperProps) {
  return (
    <HeroUIProvider>
      <PreferencesProvider
        initialPreferences={props.initialPreferences}
        cookieReadSuccess={props.cookieReadSuccess}
      >
        <RouteClientWrapperInternal
          initialWaypointString={props.initialWaypointString}
          initialModel={props.initialModel}
          initialCrossSectionDataStr={props.initialCrossSectionDataStr}
          initialWaypoints={props.initialWaypoints}
          initialElevations={props.initialElevations}
          initialRoutePoints={props.initialRoutePoints}
          initialCruiseAltitudeFt={props.initialCruiseAltitudeFt}
          initialTasKnots={props.initialTasKnots}
          initialDepartureTime={props.initialDepartureTime}
          initialResolutionNM={props.initialResolutionNM}
        />
      </PreferencesProvider>
    </HeroUIProvider>
  );
}
