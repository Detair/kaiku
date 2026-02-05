# Sound Files

These `.wav` files are served as static assets by Vite for browser/webview playback.

**Important:** The same files also exist in `../../src-tauri/resources/sounds/` where they
are embedded into the Tauri binary at compile time via `include_bytes!()`. If you add,
remove, or update a sound file, you **must** update both locations to keep them in sync.
