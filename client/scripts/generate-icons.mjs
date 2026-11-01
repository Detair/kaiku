#!/usr/bin/env node
// Regenerate the desktop app icon set and the tray icons from their sources.
//
//   bun run icons                       # uses src-tauri/icons/source/*.png
//   bun run icons -- --app my-logo.png  # one-off override of the app source
//
// To swap the artwork: overwrite src-tauri/icons/source/app-icon.png (square,
// >= 1024 px, transparent corners) and source/tray-icon.png, then run
// `bun run icons` and commit the result.
//
// App icons are produced by the official Tauri CLI (`tauri icon`). Only the
// files that tauri.conf.json (bundle.icon) references are kept; the CLI's
// Android/iOS/Windows-Store extras are discarded because no bundle target uses
// them. Tray icons and the web favicon/apple-touch icon are resized with sharp.

import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import sharp from "sharp";

const clientDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const iconsDir = join(clientDir, "src-tauri", "icons");
const trayDir = join(iconsDir, "tray");
const publicDir = join(clientDir, "public");

/** Files kept from the `tauri icon` output (must match tauri.conf.json). */
const APP_FILES = [
  "32x32.png",
  "64x64.png",
  "128x128.png",
  "128x128@2x.png",
  "icon.png",
  "icon.icns",
  "icon.ico",
];

/** Tray icon sizes (px): 32 for standard DPI, 64 for HiDPI panels. */
const TRAY_SIZES = [32, 64];

/** Entries of the web favicon.ico (px). */
const FAVICON_SIZES = [16, 32, 48];

function resizedPng(source, size) {
  return sharp(source)
    .resize(size, size, { fit: "contain", kernel: "lanczos3" })
    .png({ compressionLevel: 9 });
}

/** Pack PNG images into an .ico container (PNG-compressed entries). */
function packIco(pngs) {
  const header = Buffer.alloc(6 + pngs.length * 16);
  header.writeUInt16LE(0, 0); // reserved
  header.writeUInt16LE(1, 2); // type: icon
  header.writeUInt16LE(pngs.length, 4);
  let offset = header.length;
  pngs.forEach(({ size, data }, i) => {
    const e = 6 + i * 16;
    header[e] = size; // width (< 256)
    header[e + 1] = size; // height
    header.writeUInt16LE(1, e + 4); // colour planes
    header.writeUInt16LE(32, e + 6); // bits per pixel
    header.writeUInt32LE(data.length, e + 8);
    header.writeUInt32LE(offset, e + 12);
    offset += data.length;
  });
  return Buffer.concat([header, ...pngs.map((p) => p.data)]);
}

function argValue(flag) {
  const i = process.argv.indexOf(flag);
  return i === -1 ? undefined : process.argv[i + 1];
}

const appSource = resolve(argValue("--app") ?? join(iconsDir, "source", "app-icon.png"));
const traySource = resolve(argValue("--tray") ?? join(iconsDir, "source", "tray-icon.png"));

const work = mkdtempSync(join(tmpdir(), "kaiku-icons-"));
try {
  const run = spawnSync("bunx", ["tauri", "icon", appSource, "-o", work], {
    cwd: clientDir,
    stdio: ["ignore", "ignore", "inherit"],
  });
  if (run.status !== 0) {
    throw new Error(`tauri icon failed (exit ${run.status})`);
  }
  for (const f of APP_FILES) {
    copyFileSync(join(work, f), join(iconsDir, f));
  }

  mkdirSync(trayDir, { recursive: true });
  for (const size of TRAY_SIZES) {
    await resizedPng(traySource, size).toFile(join(trayDir, `${size}x${size}.png`));
  }

  const favicons = await Promise.all(
    FAVICON_SIZES.map(async (size) => ({ size, data: await resizedPng(appSource, size).toBuffer() })),
  );
  writeFileSync(join(publicDir, "favicon.ico"), packIco(favicons));
  await resizedPng(appSource, 180).toFile(join(publicDir, "apple-touch-icon.png"));
  console.log(`Icons written to ${iconsDir} and ${publicDir}`);
} finally {
  rmSync(work, { recursive: true, force: true });
}
