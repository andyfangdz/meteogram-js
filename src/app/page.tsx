import { redirect } from "next/navigation";

const DEFAULT_LOCATION = "KCDW";
const DEFAULT_MODEL = "gfs_hrrr";

export default function Home() {
  redirect(`/${encodeURIComponent(DEFAULT_LOCATION)}/${DEFAULT_MODEL}`);
}
