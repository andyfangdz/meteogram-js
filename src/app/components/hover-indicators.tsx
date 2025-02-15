import React from "react";
import { CloudCell } from "../../types/weather";
import { formatNumber } from "../../utils/meteogram";

interface HoverIndicatorsProps {
  date: Date;
  cloudCell: CloudCell;
  scales: {
    dateScale: any;
    mslScale: any;
  };
  bounds: {
    yMax: number;
    xMax: number;
  };
}

const HoverIndicators: React.FC<HoverIndicatorsProps> = ({
  date,
  cloudCell,
  scales,
  bounds,
}) => {
  return (
    <>
      <line
        className="hover-line hover-line-vertical"
        x1={formatNumber(scales.dateScale(date))}
        x2={formatNumber(scales.dateScale(date))}
        y1={0}
        y2={formatNumber(bounds.yMax)}
        stroke="#000000"
        strokeWidth={1}
        pointerEvents="none"
      />
      <line
        className="hover-line hover-line-horizontal"
        x1={0}
        x2={formatNumber(bounds.xMax)}
        y1={formatNumber(scales.mslScale(cloudCell.mslFt))}
        y2={formatNumber(scales.mslScale(cloudCell.mslFt))}
        stroke="#000000"
        strokeWidth={1}
        pointerEvents="none"
      />
    </>
  );
};

export default React.memo(HoverIndicators);
