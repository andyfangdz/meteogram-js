export interface CloudCell {
  hpa: number;
  mslFt: number;
  geopotentialFt: number;
  cloudCoverage: number;
  mslFtBottom: number;
  mslFtTop: number;
  temperature: number; // Temperature in Celsius
}

export interface CloudColumn {
  date: Date;
  cloud: CloudCell[];
  groundTemp: number; // Temperature in Celsius at 2m above ground
}

export interface Location {
  longitude: number;
  latitude: number;
}

export interface Locations {
  [key: string]: Location;
}

export type WeatherModel = "gfs_seamless" | "gfs_hrrr";

export interface ModelConfig {
  varsKey: string;
  stepKey: string;
  stepSize: number;
  forecastDataKey: "minutely15" | "hourly";
}

export interface ModelConfigs {
  [key in WeatherModel]: ModelConfig;
}

export interface WeatherApiParams {
  cell_selection: string;
  longitude: number;
  latitude: number;
  models: string;
  [key: string]: any;
}
