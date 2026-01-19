// Server Component that injects weather data as a script tag
// This bypasses RSC serialization by embedding the JSON directly in HTML

export function WeatherDataScript({ dataJson }: { dataJson: string }) {
  // The data is already a JSON string, so we just need to assign it
  // Using a script tag that executes immediately
  const scriptContent = `window.__WEATHER_DATA__ = ${dataJson};`;

  return (
    <script
      dangerouslySetInnerHTML={{ __html: scriptContent }}
    />
  );
}
