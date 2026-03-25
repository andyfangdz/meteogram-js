import { notFound } from "next/navigation";
import { MODEL_NAMES } from "@/config/weather";
import { WeatherModel } from "@/types/weather";
import { getInitialPreferences } from "@/utils/serverPreferences";
import { parseRouteParams } from "@/utils/params";
import {
  resolveRouteWaypoints,
  fetchRouteWeatherAction,
} from "@/app/actions/route";
import RouteClientWrapper from "@/app/components/route-client-wrapper";

export const dynamic = "force-dynamic";

interface PageProps {
  params: Promise<{
    waypoints: string;
    model: string;
  }>;
  searchParams: Promise<Record<string, string | undefined>>;
}

export default async function RoutePage({ params, searchParams }: PageProps) {
  const [{ waypoints, model }, searchParamsResolved] = await Promise.all([
    params,
    searchParams,
  ]);

  if (!MODEL_NAMES.includes(model as WeatherModel)) {
    notFound();
  }

  const decodedWaypoints = decodeURIComponent(waypoints);

  const [routeParams, preferencesResult] = await Promise.all([
    Promise.resolve(parseRouteParams(searchParamsResolved)),
    getInitialPreferences(searchParamsResolved),
  ]);

  const { cruiseAltitudeFt, tasKnots, departureTime, resolutionNM } =
    routeParams;

  const resolvedWaypoints = await resolveRouteWaypoints(
    decodedWaypoints,
    resolutionNM,
  );

  const { crossSectionData, elevations, routePoints } =
    await fetchRouteWeatherAction(
      resolvedWaypoints,
      model as WeatherModel,
      departureTime,
      cruiseAltitudeFt,
      tasKnots,
    );

  return (
    <RouteClientWrapper
      initialWaypointString={decodedWaypoints}
      initialModel={model as WeatherModel}
      initialCrossSectionDataStr={JSON.stringify(crossSectionData)}
      initialWaypoints={resolvedWaypoints}
      initialElevations={elevations}
      initialRoutePoints={routePoints.map((rp) => ({
        estimatedTimeOver: rp.estimatedTimeOver.toISOString(),
        bearingDeg: rp.bearingDeg,
      }))}
      initialCruiseAltitudeFt={cruiseAltitudeFt}
      initialTasKnots={tasKnots}
      initialDepartureTime={departureTime.toISOString()}
      initialResolutionNM={resolutionNM}
      initialPreferences={preferencesResult.preferences}
      cookieReadSuccess={preferencesResult.cookieReadSuccess}
    />
  );
}
