import { theme } from "@/stores/theme";

/**
 * Helper to get the path for theme-specific illustration images.
 * Fallbacks to 'focused-hybrid' if the current theme does not have specific assets.
 */
export function getThemeImage(imageName: string): string {
  const currentTheme = theme();
  
  // Currently, pixel-cozy does not have its own illustration variants.
  if (currentTheme === "pixel-cozy") {
    return `/themes/focused-hybrid/images/${imageName}`;
  }
  
  return `/themes/${currentTheme}/images/${imageName}`;
}
