import { parseIngestionStatus } from "@/utils/ingestion";

export async function GET() {
  try {
    const base = process.env.METEOGRAM_API_URL;
    if (!base) throw new Error("Native API not configured");
    const url = new URL("/ingestion", base);
    if (
      url.protocol !== "https:" &&
      !(
        url.protocol === "http:" &&
        ["127.0.0.1", "localhost", "[::1]"].includes(url.hostname)
      )
    )
      throw new Error("Invalid native API URL");
    const token = process.env.METEOGRAM_API_TOKEN;
    const response = await fetch(url, {
      next: { revalidate: 15 },
      signal: AbortSignal.timeout(8000),
      redirect: "error",
      headers: token ? { Authorization: `Bearer ${token}` } : {},
    });
    if (!response.ok) throw new Error("Native status unavailable");
    return Response.json(parseIngestionStatus(await response.json()), {
      headers: { "Cache-Control": "public, s-maxage=15, max-age=0" },
    });
  } catch {
    return Response.json(
      {
        error:
          "Ingestion status is temporarily unavailable. Please try again shortly.",
      },
      { status: 503, headers: { "Cache-Control": "no-store" } },
    );
  }
}
