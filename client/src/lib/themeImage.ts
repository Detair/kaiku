import { theme } from "@/stores/theme";

/**
 * Helper to get the path for theme-specific illustration images.
 *
 * Call sites pass the historical `.png` filename; the assets ship as WebP
 * (~15× smaller than the source PNGs — see perf/frontend-image-loading), so
 * the extension is normalised to `.webp` here. This keeps every call site
 * unchanged while serving the optimised format.
 *
 * Falls back to 'focused-hybrid' if the current theme has no specific assets.
 */
export function getThemeImage(imageName: string): string {
  const webpName = imageName.replace(/\.png$/i, ".webp");
  const currentTheme = theme();

  // Currently, pixel-cozy does not have its own illustration variants.
  const themeDir =
    currentTheme === "pixel-cozy" ? "focused-hybrid" : currentTheme;

  return `/themes/${themeDir}/images/${webpName}`;
}
