import {
  Locations,
  LocationWithDescription,
  LocationsWithDescription,
} from "../types/weather";

const NOMINATIM_URL = "https://nominatim.openstreetmap.org/search";

interface NominatimResult {
  display_name: string;
  lat: string;
  lon: string;
}

// Format coordinate to 6 decimal places
function formatCoordinate(coord: number): number {
  return Number(coord.toFixed(6));
}

export async function geocodeLocation(
  query: string,
): Promise<LocationsWithDescription> {
  // If it's an ICAO code (starts with K and 4 characters), search specifically for airports
  const isICAO = /^K[A-Z]{3}$/.test(query);
  const searchQuery = isICAO ? `${query} airport` : query;

  const params = new URLSearchParams({
    q: searchQuery,
    format: "json",
    limit: "5", // Get more results to offer more choices
  });

  try {
    const response = await fetch(`${NOMINATIM_URL}?${params}`);
    const results: NominatimResult[] = await response.json();

    if (results.length > 0) {
      const locations: LocationsWithDescription = {};

      results.forEach((result, index) => {
        // Create a unique key if needed
        const locationKey = index === 0 ? query : `${query} (${index})`;

        locations[locationKey] = {
          latitude: formatCoordinate(parseFloat(result.lat)),
          longitude: formatCoordinate(parseFloat(result.lon)),
          description: result.display_name,
        };
      });

      return locations;
    }
    return {};
  } catch (error) {
    console.error("Geocoding error:", error);
    return {};
  }
}

// Debounce function to limit API calls
function debounce<T extends (...args: any[]) => Promise<any>>(
  func: T,
  wait: number,
): (...args: Parameters<T>) => Promise<Awaited<ReturnType<T>>> {
  let timeout: NodeJS.Timeout;
  return (...args: Parameters<T>) => {
    return new Promise((resolve) => {
      clearTimeout(timeout);
      timeout = setTimeout(() => resolve(func(...args)), wait);
    });
  };
}

// Create a debounced version of geocodeLocation
export const debouncedGeocodeLocation = debounce(geocodeLocation, 300);
