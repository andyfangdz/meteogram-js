import React from "react";

// Format numbers to ensure consistent precision
const formatNumber = (num: number) => {
  const formatted = Number(num.toFixed(4));
  return Number.isFinite(formatted) ? formatted : 0;
};

// Helper function to calculate point on circle
const pointOnCircle = (
  centerX: number,
  centerY: number,
  radius: number,
  angleDegrees: number,
) => {
  const angleRadians = ((angleDegrees - 90) * Math.PI) / 180.0;
  return {
    x: formatNumber(centerX + radius * Math.cos(angleRadians)),
    y: formatNumber(centerY + radius * Math.sin(angleRadians)),
  };
};

interface WindBarbProps {
  x: number;
  y: number;
  speed: number; // Speed in kilometers per hour
  direction: number; // Direction in degrees (0-360)
  size?: number; // Size of the barb in pixels
}

// Convert km/h to knots
const kmhToKnots = (kmh: number) => kmh * 0.539957;

// Calculate endpoints for barb lines based on wind direction
const getEndpoint = (x: number, y: number, angle: number, length: number) => {
  if (
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !Number.isFinite(angle) ||
    !Number.isFinite(length)
  ) {
    return { x: 0, y: 0 };
  }
  const radians = (angle - 90) * (Math.PI / 180); // -90 to align with meteorological convention
  return {
    x: formatNumber(x + length * Math.cos(radians)),
    y: formatNumber(y + length * Math.sin(radians)),
  };
};

export default function WindBarb({
  x,
  y,
  speed,
  direction,
  size = 20,
}: WindBarbProps) {
  // Validate input parameters
  if (
    !Number.isFinite(x) ||
    !Number.isFinite(y) ||
    !Number.isFinite(speed) ||
    !Number.isFinite(direction)
  ) {
    return null;
  }

  const knots = kmhToKnots(speed);
  const flags = Math.floor(knots / 50); // 50 knot flags
  const longBarbs = Math.floor((knots % 50) / 10); // 10 knot barbs
  const shortBarb = Math.floor((knots % 10) / 5) === 1; // 5 knot half barb

  // Calculate the main staff line
  const staffEnd = getEndpoint(x, y, direction, size);
  if (!staffEnd) return null;

  // Generate the path for the wind barb
  let path = `M ${formatNumber(x)} ${formatNumber(y)} L ${formatNumber(staffEnd.x)} ${formatNumber(staffEnd.y)}`;

  // Starting point for barbs, from the end of the staff
  let currentPoint = { x: staffEnd.x, y: staffEnd.y };
  const barbSpacing = size / 8;
  const barbLength = size / 3;

  // Add flags (50 knots each)
  for (let i = 0; i < flags; i++) {
    const flagStart = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction - 90,
      barbLength,
    );
    const flagEnd = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction,
      -barbSpacing,
    );

    if (!flagStart || !flagEnd) continue;

    const trianglePath = `
      M ${formatNumber(currentPoint.x)} ${formatNumber(currentPoint.y)}
      L ${formatNumber(flagStart.x)} ${formatNumber(flagStart.y)}
      L ${formatNumber(flagEnd.x)} ${formatNumber(flagEnd.y)}
      Z
    `;
    path += trianglePath;
    currentPoint = flagEnd;
  }

  // Add long barbs (10 knots each)
  for (let i = 0; i < longBarbs; i++) {
    const barbEnd = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction - 90,
      barbLength,
    );

    if (!barbEnd) continue;

    path += ` M ${formatNumber(currentPoint.x)} ${formatNumber(currentPoint.y)} L ${formatNumber(barbEnd.x)} ${formatNumber(barbEnd.y)}`;
    currentPoint = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction,
      -barbSpacing,
    );
    if (!currentPoint) break;
  }

  // Add short barb (5 knots) if needed
  if (shortBarb) {
    const barbEnd = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction - 90,
      barbLength / 2,
    );

    if (barbEnd) {
      path += ` M ${formatNumber(currentPoint.x)} ${formatNumber(currentPoint.y)} L ${formatNumber(barbEnd.x)} ${formatNumber(barbEnd.y)}`;
    }
  }

  return (
    <path
      d={path}
      stroke="black"
      strokeWidth={1}
      fill="none"
      strokeLinecap="round"
      strokeLinejoin="round"
    />
  );
}
