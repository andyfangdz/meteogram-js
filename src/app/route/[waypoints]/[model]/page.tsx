import { notFound } from "next/navigation";
import { MODEL_NAMES } from "@/config/weather";
import { WeatherModel } from "@/types/weather";
import { getInitialPreferences } from "@/utils/serverPreferences";
import { parseRouteParams } from "@/utils/params";
import {
  resolveRouteWaypoints,
  fetchRouteWeatherAction,
} from "@/app/actions/route-actions";
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

  let resolvedWaypoints;
  try {
    resolvedWaypoints = await resolveRouteWaypoints(
      decodedWaypoints,
      resolutionNM,
    );
  } catch (error) {
    console.error("Failed to resolve waypoints:", error);
    return (
      <div className="min-h-screen flex items-center justify-center p-4">
        <div className="text-center">
          <h1 className="text-xl font-bold mb-2">Could not resolve route</h1>
          <p className="text-default-500">
            {error instanceof Error ? error.message : "Unknown error resolving waypoints"}
          </p>
          <p className="text-default-400 mt-2">
            Check that all waypoints are valid airport codes, navaid identifiers, or Name@lat,lon format.
          </p>
        </div>
      </div>
    );
  }

  let crossSectionData, elevations, routePoints;
  try {
    const result = await fetchRouteWeatherAction(
      resolvedWaypoints,
      model as WeatherModel,
      departureTime,
      cruiseAltitudeFt,
      tasKnots,
    );
    crossSectionData = result.crossSectionData;
    elevations = result.elevations;
    routePoints = result.routePoints;
  } catch (error) {
    console.error("Failed to fetch route weather:", error);
    return (
      <div className="min-h-screen flex items-center justify-center p-4">
        <div className="text-center">
          <h1 className="text-xl font-bold mb-2">Could not fetch weather data</h1>
          <p className="text-default-500">
            {error instanceof Error ? error.message : "Unknown error fetching weather"}
          </p>
          <p className="text-default-400 mt-2">
            The departure time may be outside the forecast range for this model.
            Try a closer date or a different model (e.g., gfs_seamless for longer range).
          </p>
        </div>
      </div>
    );
  }

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
