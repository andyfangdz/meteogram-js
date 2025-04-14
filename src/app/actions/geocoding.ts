"use server";

import { LocationsWithDescription } from "../../types/weather"; // Assuming types are moved or redefined

const NOMINATIM_URL = "https://nominatim.openstreetmap.org/search";
const AIRPORTS_API_URL =
  "https://find-an-approach.github.io/data/approaches.json";

// In-memory cache for airports data (server-side)
let airportsCache: Record<
  string,
  { latitude: number; longitude: number; description: string }
> = {};
let isCacheInitialized = false;

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
  if (!dms || dms.length < 1) return NaN; // Basic validation

  let sign = 1; // Default to N/E
  const direction = dms.charAt(0).toUpperCase();

  if (direction === "S" || direction === "W") {
    sign = -1;
  } else if (direction !== "N" && direction !== "E") {
    console.warn(`Invalid direction character: ${direction} in ${dms}`);
    // Optionally handle error differently, e.g., return NaN or throw
  }

  const coordString = dms.substring(1).padStart(9, "0"); // Ensure padding after removing direction

  const degrees = parseFloat(coordString.substring(0, 3));
  const minutes = parseFloat(coordString.substring(3, 5));
  const seconds = parseFloat(coordString.substring(5, 9)) / 100; // Assuming last 4 are seconds.xx

  if (isNaN(degrees) || isNaN(minutes) || isNaN(seconds)) {
    console.error(`Failed to parse ARINC424 coordinate: ${dms}`);
    return NaN; // Return NaN if parsing fails
  }

  return sign * (degrees + minutes / 60 + seconds / (60 * 60));
}

// Initialize airports cache (server-side)
async function initAirportsCache() {
  // Avoid re-initialization in the same server instance lifetime
  if (isCacheInitialized) return;

  console.log("Initializing server-side airports cache...");
  try {
    const response = await fetch(AIRPORTS_API_URL, {
      next: { revalidate: 3600 * 24 },
    }); // Revalidate once a day
    if (!response.ok) {
      console.error("Failed to fetch airports data:", response.statusText);
      isCacheInitialized = false; // Allow retry on next call
      return;
    }

    const data = await response.json();
    const newCache: typeof airportsCache = {};

    for (const [airportId, airportData] of Object.entries(
      data.airports || {},
    )) {
      const airport = airportData as any;

      if (airport.id && airport.latitude && airport.longitude) {
        const id = airport.id.toUpperCase();
        const lat = convertArinc424ToDegrees(airport.latitude);
        const lon = convertArinc424ToDegrees(airport.longitude);

        if (!isNaN(lat) && !isNaN(lon)) {
          newCache[id] = {
            latitude: formatCoordinate(lat),
            longitude: formatCoordinate(lon),
            description: `${airport.name || "Unknown Name"} Airport (${id})`, // Add fallback for name
          };
        } else {
          console.warn(
            `Skipping airport ${id} due to invalid coordinate conversion: lat=${airport.latitude}, lon=${airport.longitude}`,
          );
        }
      }
    }
    airportsCache = newCache; // Atomically update cache
    isCacheInitialized = true; // Mark as initialized
    console.log(
      `Loaded ${Object.keys(airportsCache).length} airports into server cache`,
    );
  } catch (error) {
    console.error("Failed to initialize server-side airports cache:", error);
    isCacheInitialized = false; // Allow retry on next call
  }
}

export async function geocodeLocationAction(
  query: string,
): Promise<LocationsWithDescription> {
  // Ensure cache is initialized (awaits if first call)
  await initAirportsCache();

  const locations: LocationsWithDescription = {};
  const normalizedQuery = query.trim().toUpperCase();

  // 1. Check Airport Cache
  if (normalizedQuery) {
    // Direct match
    if (airportsCache[normalizedQuery]) {
      locations[normalizedQuery] = airportsCache[normalizedQuery];
      return locations; // Return immediately if direct airport match found
    }
    // US 3-letter with K prefix
    if (normalizedQuery.length === 3 && /^[A-Z0-9]+$/.test(normalizedQuery)) {
      const withK = "K" + normalizedQuery;
      if (airportsCache[withK]) {
        locations[withK] = airportsCache[withK];
        return locations; // Return immediately if K-prefix match found
      }
    }
  }

  // 2. If no airport match, query Nominatim
  if (query) {
    // Use original query for Nominatim
    const isICAO = /^[A-Z]{4}$/.test(normalizedQuery);
    // If it looks like an ICAO code, append " airport" for better Nominatim results
    const searchQuery = isICAO ? `${normalizedQuery} airport` : query;

    const params = new URLSearchParams({
      q: searchQuery,
      format: "json",
      limit: "5",
      "accept-language": "en", // Optional: prefer English results
    });

    try {
      const response = await fetch(`${NOMINATIM_URL}?${params}`);
      if (!response.ok) {
        console.error(
          `Nominatim request failed: ${response.status} ${response.statusText}`,
        );
        // Decide if you want to throw an error or return empty locations
        return locations; // Return empty or partially filled locations
      }
      const results: NominatimResult[] = await response.json();

      if (results.length > 0) {
        results.forEach((result, index) => {
          // Use a more descriptive key if possible, fallback to index
          const locationKey = result.display_name || `${query} (${index + 1})`; // Use 1-based index for user display

          locations[locationKey] = {
            latitude: formatCoordinate(parseFloat(result.lat)),
            longitude: formatCoordinate(parseFloat(result.lon)),
            description: result.display_name,
          };
        });
      }
    } catch (error) {
      console.error("Geocoding error (Nominatim):", error);
      // Decide how to handle fetch errors - potentially return empty locations
    }
  }

  // Return whatever was found (could be empty)
  return locations;
}

// Note: Debouncing should now happen on the client-side before calling this action.
