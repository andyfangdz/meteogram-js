export interface CloudCell {
  hpa: number;
  mslFt: number;
  geopotentialFt: number;
  cloudCoverage: number;
  mslFtBottom: number;
  mslFtTop: number;
  temperature: number; // Temperature in Celsius
  windSpeed: number; // Wind speed in meters per second
  windDirection: number; // Wind direction in degrees (0-360)
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

export type WeatherModel =
  | "gfs_seamless"
  | "gfs_hrrr"
  | "ecmwf_ifs025"
  | "ecmwf_aifs025";

export interface ModelConfig {
  varsKey: string;
  stepKey: string;
  stepSize: number;
  forecastDataKey: "minutely15" | "hourly";
  windBarbStep: number; // How many time steps between wind barbs
  windBarbPressureLevelStep: number; // How many pressure levels to skip between wind barbs
  maxIsothermStepDistance: number; // Maximum number of forecast steps an isotherm can cross
}

export interface ModelConfigs {
  gfs_hrrr: ModelConfig;
  gfs_seamless: ModelConfig;
  ecmwf_ifs025: ModelConfig;
  ecmwf_aifs025: ModelConfig;
}

export interface WeatherApiParams {
  cell_selection: string;
  longitude: number;
  latitude: number;
  models: string;
  [key: string]: any;
}
