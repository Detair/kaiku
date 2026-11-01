// @vitest-environment node
import { readdirSync } from "node:fs";
import { join } from "node:path";
import sharp from "sharp";
import { describe, expect, it } from "vitest";

/**
 * Mascot illustrations are drawn on themed (dark/light) backgrounds, so they
 * must ship with a transparent background. An opaque white box around the
 * subject looks broken on dark themes.
 */
const ASSET_DIRS = ["emotes", "images"].map((d) => join(__dirname, "..", d));
const RASTER = /\.(webp|png)$/i;

const files = ASSET_DIRS.flatMap((dir) =>
  readdirSync(dir)
    .filter((f) => RASTER.test(f))
    .map((f) => join(dir, f)),
);

describe("mascot assets have transparent backgrounds", () => {
  it("finds assets to check", () => {
    expect(files.length).toBeGreaterThan(0);
  });

  it.each(files.map((f) => [f.split("/assets/")[1], f]))(
    "%s has alpha 0 at all four corners",
    async (_name, file) => {
      const { data, info } = await sharp(file)
        .ensureAlpha()
        .raw()
        .toBuffer({ resolveWithObject: true });
      const { width, height, channels } = info;
      const alphaAt = (x: number, y: number) =>
        data[(y * width + x) * channels + (channels - 1)];
      const corners = [
        alphaAt(0, 0),
        alphaAt(width - 1, 0),
        alphaAt(0, height - 1),
        alphaAt(width - 1, height - 1),
      ];
      expect(corners).toEqual([0, 0, 0, 0]);
    },
  );
});
