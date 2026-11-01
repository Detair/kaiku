// @vitest-environment node
// Guard against placeholder app/tray icons (1x1 PNGs, empty .icns, stub .ico).
import { existsSync, readFileSync, statSync } from "node:fs";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";

const tauriDir = resolve(dirname(fileURLToPath(import.meta.url)), "../../src-tauri");
const conf = JSON.parse(readFileSync(join(tauriDir, "tauri.conf.json"), "utf8"));
const bundleIcons: string[] = conf.bundle.icon;

const PNG_MAGIC = Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]);

function pngSize(path: string): { w: number; h: number } {
  const buf = readFileSync(path);
  expect(buf.subarray(0, 8).equals(PNG_MAGIC), `${path} is not a PNG`).toBe(true);
  // IHDR: width/height are big-endian u32 at offsets 16 and 20.
  return { w: buf.readUInt32BE(16), h: buf.readUInt32BE(20) };
}

/** Expected pixel size from names like `32x32.png` or `128x128@2x.png`. */
function nominalSize(file: string): number | undefined {
  const m = /^(\d+)x\1(@2x)?\.png$/.exec(file);
  return m ? Number(m[1]) * (m[2] ? 2 : 1) : undefined;
}

describe("desktop app icons", () => {
  it("references at least one icon of each platform format", () => {
    expect(bundleIcons.some((f) => f.endsWith(".png"))).toBe(true);
    expect(bundleIcons.some((f) => f.endsWith(".icns"))).toBe(true);
    expect(bundleIcons.some((f) => f.endsWith(".ico"))).toBe(true);
  });

  it.each(bundleIcons)("%s exists and is non-empty", (rel) => {
    const path = join(tauriDir, rel);
    expect(existsSync(path), `${rel} missing`).toBe(true);
    expect(statSync(path).size).toBeGreaterThan(0);
  });

  it.each(bundleIcons.filter((f) => f.endsWith(".png")))("%s matches its nominal size", (rel) => {
    const { w, h } = pngSize(join(tauriDir, rel));
    expect(w).toBe(h);
    const nominal = nominalSize(rel.split("/").pop() as string);
    if (nominal !== undefined) {
      expect(w).toBe(nominal);
    } else {
      // e.g. icon.png: the large Linux icon must not be a tiny placeholder.
      expect(w).toBeGreaterThanOrEqual(256);
    }
  });

  it.each(bundleIcons.filter((f) => f.endsWith(".icns")))("%s is a real icns", (rel) => {
    const buf = readFileSync(join(tauriDir, rel));
    expect(buf.length).toBeGreaterThan(10 * 1024);
    expect(buf.subarray(0, 4).toString("ascii")).toBe("icns");
    expect(buf.readUInt32BE(4)).toBe(buf.length);
  });

  it.each(bundleIcons.filter((f) => f.endsWith(".ico")))("%s has 16/32/48/256 entries", (rel) => {
    const buf = readFileSync(join(tauriDir, rel));
    expect(buf.readUInt16LE(0)).toBe(0); // reserved
    expect(buf.readUInt16LE(2)).toBe(1); // type: icon
    const count = buf.readUInt16LE(4);
    const sizes = new Set<number>();
    for (let i = 0; i < count; i++) {
      const w = buf[6 + i * 16]; // 0 means 256
      sizes.add(w === 0 ? 256 : w);
    }
    for (const s of [16, 32, 48, 256]) {
      expect(sizes.has(s), `missing ${s}px entry`).toBe(true);
    }
  });
});

describe("tray icons", () => {
  it.each([32, 64])("icons/tray/%ix%i.png exists with the right size", (size) => {
    const path = join(tauriDir, "icons", "tray", `${size}x${size}.png`);
    expect(existsSync(path)).toBe(true);
    expect(pngSize(path)).toEqual({ w: size, h: size });
  });

  it("keeps the committed source artwork at >= 1024px", () => {
    for (const name of ["app-icon.png", "tray-icon.png"]) {
      const { w, h } = pngSize(join(tauriDir, "icons", "source", name));
      expect(w).toBe(h);
      expect(w).toBeGreaterThanOrEqual(1024);
    }
  });
});
