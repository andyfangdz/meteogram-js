import { Locations } from "../types/weather";

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

export async function geocodeLocation(query: string): Promise<Locations> {
  // If it's an ICAO code (starts with K and 4 characters), search specifically for airports
  const isICAO = /^K[A-Z]{3}$/.test(query);
  const searchQuery = isICAO ? `${query} airport` : query;

  const params = new URLSearchParams({
    q: searchQuery,
    format: "json",
    limit: "1",
  });

  try {
    const response = await fetch(`${NOMINATIM_URL}?${params}`);
    const results: NominatimResult[] = await response.json();

    if (results.length > 0) {
      return {
        [query]: {
          latitude: formatCoordinate(parseFloat(results[0].lat)),
          longitude: formatCoordinate(parseFloat(results[0].lon)),
        },
      };
    }
    return {};
  } catch (error) {
    console.error("Geocoding error:", error);
    return {};
  }
}

// Debounce function to limit API calls
export function debounce<T extends (...args: any[]) => any>(
  func: T,
  wait: number,
): (...args: Parameters<T>) => void {
  let timeout: NodeJS.Timeout;
  return (...args: Parameters<T>) => {
    clearTimeout(timeout);
    timeout = setTimeout(() => func(...args), wait);
  };
}
