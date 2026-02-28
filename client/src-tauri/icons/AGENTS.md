<!-- Parent: ../AGENTS.md -->
# Application Icons

## Purpose

Application icons for the Tauri desktop client. Platform-specific icon formats for taskbar, dock, window chrome, and installers.

## Key Files

| File | Purpose |
|------|---------|
| `32x32.png` | Small icon (taskbar, system tray) |
| `128x128.png` | Standard icon (application switcher) |
| `128x128@2x.png` | Retina/HiDPI icon (macOS 2x displays) |
| `icon.icns` | macOS application bundle icon |
| `icon.ico` | Windows application icon |

## For AI Agents

### Icon Requirements

**PNG icons:**
- Square dimensions
- Transparent background supported
- Standard sizes: 32x32, 128x128, 256x256

**macOS (`icon.icns`):**
- Contains multiple resolutions (16x16 to 1024x1024)
- Generate with `iconutil` or online tools
- Required for .app bundles

**Windows (`icon.ico`):**
- Contains multiple resolutions (16x16 to 256x256)
- Generate with ImageMagick or online tools
- Required for .exe installers

### Regenerating Icons

From a source image (1024x1024 recommended):

```bash
# Install ImageMagick
brew install imagemagick  # macOS
apt install imagemagick   # Linux

# Generate PNGs
convert source.png -resize 32x32 icons/32x32.png
convert source.png -resize 128x128 icons/128x128.png
convert source.png -resize 256x256 icons/128x128@2x.png

# Generate ICO (Windows)
convert source.png -define icon:auto-resize=256,128,64,48,32,16 icons/icon.ico

# Generate ICNS (macOS) - requires iconutil
mkdir icon.iconset
sips -z 16 16 source.png --out icon.iconset/icon_16x16.png
sips -z 32 32 source.png --out icon.iconset/icon_32x32.png
# ... (repeat for all sizes)
iconutil -c icns icon.iconset -o icons/icon.icns
```

### Tauri Configuration

Icons are referenced in `tauri.conf.json`:

```json
{
  "bundle": {
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns",
      "icons/icon.ico"
    ]
  }
}
```

### Current Status

Icons are placeholder files. Replace with actual Kaiku branding before production release.

## Dependencies

- Tauri build system reads these during `cargo tauri build`
- ImageMagick or similar for generation
