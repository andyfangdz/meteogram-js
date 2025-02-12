import React from "react";

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
  const radians = (angle - 90) * (Math.PI / 180); // -90 to align with meteorological convention
  return {
    x: x + length * Math.cos(radians),
    y: y + length * Math.sin(radians),
  };
};

export default function WindBarb({
  x,
  y,
  speed,
  direction,
  size = 20,
}: WindBarbProps) {
  const knots = kmhToKnots(speed);
  const flags = Math.floor(knots / 50); // 50 knot flags
  const longBarbs = Math.floor((knots % 50) / 10); // 10 knot barbs
  const shortBarb = Math.floor((knots % 10) / 5) === 1; // 5 knot half barb

  // Calculate the main staff line
  const staffEnd = getEndpoint(x, y, direction, size);

  // Generate the path for the wind barb
  let path = `M ${x} ${y} L ${staffEnd.x} ${staffEnd.y}`;

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
    const trianglePath = `
      M ${currentPoint.x} ${currentPoint.y}
      L ${flagStart.x} ${flagStart.y}
      L ${getEndpoint(currentPoint.x, currentPoint.y, direction, -barbSpacing).x}
      ${getEndpoint(currentPoint.x, currentPoint.y, direction, -barbSpacing).y}
      Z
    `;
    path += trianglePath;
    currentPoint = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction,
      -barbSpacing,
    );
  }

  // Add long barbs (10 knots each)
  for (let i = 0; i < longBarbs; i++) {
    const barbEnd = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction - 90,
      barbLength,
    );
    path += ` M ${currentPoint.x} ${currentPoint.y} L ${barbEnd.x} ${barbEnd.y}`;
    currentPoint = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction,
      -barbSpacing,
    );
  }

  // Add short barb (5 knots) if needed
  if (shortBarb) {
    const barbEnd = getEndpoint(
      currentPoint.x,
      currentPoint.y,
      direction - 90,
      barbLength / 2,
    );
    path += ` M ${currentPoint.x} ${currentPoint.y} L ${barbEnd.x} ${barbEnd.y}`;
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
