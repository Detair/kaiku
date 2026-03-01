# Installing System Dependencies for Tauri

## What's Missing

Your build is failing because Tauri requires system-level libraries for Linux GUI applications. Specifically:
- **GLib** - Core application building blocks
- **GTK** - GUI toolkit
- **WebKit2GTK** - Web rendering engine (used by Tauri)
- **pkg-config** - Tool for managing library compile/link flags

## Quick Fix (Ubuntu/Debian/WSL)

Run these commands in your terminal:

```bash
# Update package lists
sudo apt-get update

# Install Tauri prerequisites
sudo apt-get install -y \
    libwebkit2gtk-4.0-dev \
    build-essential \
    curl \
    wget \
    file \
    libssl-dev \
    libgtk-3-dev \
    libayatana-appindicator3-dev \
    librsvg2-dev \
    pkg-config
```

**Time required**: 2-5 minutes (depending on your connection speed)

## Verify Installation

After installation, check that pkg-config can find glib:

```bash
pkg-config --modversion glib-2.0
# Should output something like: 2.64.6
```

## Retry Build

Once dependencies are installed:

```bash
cd /home/detair/GIT/canis
cargo clean
cargo build --release
```

---

## Understanding the Dependencies

| Package | Purpose |
|---------|---------|
| `libwebkit2gtk-4.0-dev` | Web rendering engine (core Tauri requirement) |
| `build-essential` | GCC, make, and other build tools |
| `libssl-dev` | OpenSSL development headers (for TLS) |
| `libgtk-3-dev` | GTK 3 development headers (GUI toolkit) |
| `libayatana-appindicator3-dev` | System tray support |
| `librsvg2-dev` | SVG icon rendering |
| `pkg-config` | Tool for locating libraries |

---

## Alternative: Install Only What's Needed Right Now

If you want a minimal install to just fix the current error:

```bash
sudo apt-get install -y pkg-config libglib2.0-dev
```

But you'll likely need the full set above for a complete Tauri build.

---

## Platform-Specific Notes

### WSL2 (Windows Subsystem for Linux)
You're running WSL2. The Tauri **GUI won't display** in WSL2 by default (no X11 server). However, you can still:
- Build the Tauri app (cross-compile for Windows)
- Run the server component
- Develop the frontend with `bun run dev`

### To Run Tauri GUI in WSL2
Install an X11 server on Windows:
1. Install [VcXsrv](https://sourceforge.net/projects/vcxsrv/)
2. Start XLaunch with "Disable access control" checked
3. Set DISPLAY variable:
   ```bash
   export DISPLAY=$(cat /etc/resolv.conf | grep nameserver | awk '{print $2}'):0
   ```

Or use **WSLg** (Windows 11 only) which includes GUI support by default.

---

## Troubleshooting

### Error: "E: Unable to locate package"
Your package lists may be outdated:
```bash
sudo apt-get update
```

### Error: "Package 'libwebkit2gtk-4.0-dev' has no installation candidate"
Try the older package name:
```bash
sudo apt-get install libwebkit2gtk-4.1-dev
```

### Error: Still can't find glib after install
Check pkg-config path:
```bash
pkg-config --variable pc_path pkg-config
# Should show paths like /usr/lib/x86_64-linux-gnu/pkgconfig
```

### Alternative: Build Server Only
If you only need the server component (not the Tauri desktop client):
```bash
cd /home/detair/GIT/canis/server
cargo build --release
```

This won't require any GTK dependencies.

---

## Next Steps After Installing Dependencies

1. ✅ Install dependencies (commands above)
2. ✅ Verify pkg-config works
3. ✅ Run `cargo clean`
4. ✅ Run `cargo build --release`
5. ✅ Test server: `cargo run --release` (in server directory)
6. ✅ Test frontend: `bun run dev` (in client directory)

---

## Quick Command Summary

```bash
# 1. Install dependencies
sudo apt-get update
sudo apt-get install -y libwebkit2gtk-4.0-dev build-essential curl wget file libssl-dev libgtk-3-dev libayatana-appindicator3-dev librsvg2-dev pkg-config

# 2. Verify
pkg-config --modversion glib-2.0

# 3. Build
cd /home/detair/GIT/canis
cargo clean
cargo build --release

# If successful, build server specifically
cd server
cargo build --release

# If successful, build client
cd ../client
bun install
bun run build
```

---

*This is a one-time setup. Once installed, you won't need to do this again.*
