declare module "wgs84" {
  /**
   * WGS84 equatorial radius in meters (semi-major axis)
   */
  export const RADIUS: number;

  /**
   * WGS84 flattening parameter
   */
  export const FLATTENING: number;

  /**
   * WGS84 polar radius in meters (semi-minor axis)
   */
  export const POLAR_RADIUS: number;
}
