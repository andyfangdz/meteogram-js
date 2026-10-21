import type { Metadata } from "next";
import IngestionDashboard from "./dashboard";
export const metadata: Metadata = {
  title: "Ingestion status · Meteogram",
  description:
    "Live forecast ingestion progress for HRRR, GFS, AIFS, RRFS, IFS and AIGFS.",
};
export default function IngestionPage() {
  return <IngestionDashboard />;
}
