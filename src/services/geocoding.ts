import {
  Locations,
  LocationWithDescription,
  LocationsWithDescription,
} from "../types/weather";

const NOMINATIM_URL = "https://nominatim.openstreetmap.org/search";
const AIRPORTS_API_URL =
  "https://find-an-approach.github.io/data/approaches.json";

// In-memory cache for airports data
let airportsCache: Record<
  string,
  { latitude: number; longitude: number; description: string }
> = {};

interface NominatimResult {
  display_name: string;
  lat: string;
  lon: string;
}

// Format coordinate to 6 decimal places
function formatCoordinate(coord: number): number {
  return Number(coord.toFixed(6));
}

/**
 * Convert ARINC424 coordinate format (e.g., "N32244080" or "W099405480") to decimal degrees
 */
function convertArinc424ToDegrees(dms: string): number {
  let sign = -1;
  if (dms.startsWith("N") || dms.startsWith("E")) {
    sign = 1;
  }
  dms = dms.substring(1);

  // Pad to 9 decimal digits, from the left
  if (dms.length < 9) {
    dms = "0" + dms;
  }

  const degrees = parseFloat(dms.substring(0, 3));
  const minutes = parseFloat(dms.substring(3, 5));
  const seconds = parseFloat(dms.substring(5, 9)) / 100;

  return sign * (degrees + minutes / 60 + seconds / (60 * 60));
}

// Initialize airports cache
async function initAirportsCache() {
  if (Object.keys(airportsCache).length > 0) return; // Already initialized

  try {
    // Fetch airport data directly from find-an-approach API
    const response = await fetch(AIRPORTS_API_URL);
    if (!response.ok) {
      console.error("Failed to fetch airports data:", response.statusText);
      return;
    }

    const data = await response.json();

    // Process airports and add to cache
    for (const [airportId, airportData] of Object.entries(
      data.airports || {},
    )) {
      const airport = airportData as any;

      if (airport.id && airport.latitude && airport.longitude) {
        const id = airport.id.toUpperCase();
        airportsCache[id] = {
          latitude: formatCoordinate(
            convertArinc424ToDegrees(airport.latitude),
          ),
          longitude: formatCoordinate(
            convertArinc424ToDegrees(airport.longitude),
          ),
          description: `${airport.name} Airport (${id})`,
        };
      }
    }
    console.log(
      `Loaded ${Object.keys(airportsCache).length} airports into cache`,
    );
  } catch (error) {
    console.error("Failed to initialize airports cache:", error);
  }
}

// Initialize cache when module loads
initAirportsCache();

export async function geocodeLocation(
  query: string,
): Promise<LocationsWithDescription> {
  const locations: LocationsWithDescription = {};

  // Try to match airport code (case insensitive)
  if (query) {
    const normalizedQuery = query.trim().toUpperCase();

    // Check direct match with airport code
    if (airportsCache[normalizedQuery]) {
      locations[normalizedQuery] = airportsCache[normalizedQuery];
      return locations;
    }

    // For US airports, try prepending K if it's a 3-letter code
    if (normalizedQuery.length === 3) {
      const withK = "K" + normalizedQuery;
      if (airportsCache[withK]) {
        locations[withK] = airportsCache[withK];
        return locations;
      }
    }

    // Check if it's an ICAO code (4 letters) and looks for an airport
    const isICAO = /^[A-Z]{4}$/.test(normalizedQuery);
    const searchQuery = isICAO ? `${normalizedQuery} airport` : query;

    const params = new URLSearchParams({
      q: searchQuery,
      format: "json",
      limit: "5", // Get more results to offer more choices
    });

    try {
      const response = await fetch(`${NOMINATIM_URL}?${params}`);
      const results: NominatimResult[] = await response.json();

      if (results.length > 0) {
        results.forEach((result, index) => {
          // Create a unique key if needed
          const locationKey = index === 0 ? query : `${query} (${index})`;

          locations[locationKey] = {
            latitude: formatCoordinate(parseFloat(result.lat)),
            longitude: formatCoordinate(parseFloat(result.lon)),
            description: result.display_name,
          };
        });
      }
    } catch (error) {
      console.error("Geocoding error:", error);
    }
  }

  return locations;
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
