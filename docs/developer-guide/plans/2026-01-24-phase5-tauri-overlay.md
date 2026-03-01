# Design: Tauri Game Overlay & System Tray (Phase 5)

## 1. Overview
Implement a "Widget" style overlay that stays on top of games and a System Tray for background control.

## 2. System Tray Implementation

### 2.1. Tauri Config (`client/src-tauri/tauri.conf.json`)
```json
{
  "tauri": {
    "systemTray": {
      "iconPath": "icons/icon.png",
      "iconAsTemplate": true,
      "menuOnLeftClick": false
    }
  }
}
```

### 2.2. Rust Handler (`client/src-tauri/src/main.rs`)
Update `main` to build the tray:
```rust
let tray = SystemTray::new()
    .with_menu(SystemTrayMenu::new()
        .add_item(CustomMenuItem::new("toggle", "Show/Hide"))
        .add_item(CustomMenuItem::new("mute", "Mute Mic"))
        .add_item(CustomMenuItem::new("quit", "Quit")));

tauri::Builder::default()
    .system_tray(tray)
    .on_system_tray_event(|app, event| { ... })
```

## 3. Overlay Window Implementation

### 3.1. Config (`tauri.conf.json`)
Add a second window definition:
```json
"windows": [
  { "label": "main", ... },
  {
    "label": "overlay",
    "url": "overlay.html",
    "transparent": true,
    "decorations": false,
    "alwaysOnTop": true,
    "skipTaskbar": true,
    "resizable": false,
    "width": 300,
    "height": 400,
    "x": 20,
    "y": 20
  }
]
```

### 3.2. Click-Through Logic (Windows)
We need a custom command to toggle the window's "Input Transparency".

File: `client/src-tauri/src/overlay.rs`
```rust
#[tauri::command]
fn set_overlay_input_passthrough(window: Window, ignore: bool) {
    #[cfg(target_os = "windows")]
    {
        let hwnd = window.hwnd().unwrap();
        // Use user32::SetWindowLongPtrA to add/remove WS_EX_TRANSPARENT
        // WS_EX_LAYERED (0x80000) | WS_EX_TRANSPARENT (0x20)
    }
}
```

### 3.3. Global Shortcut
Use `tauri-plugin-global-shortcut` to listen for `Shift+F1` (example) to toggle the `ignore` state.
*   **Default:** `ignore=true` (Click-through, see-only).
*   **Active:** `ignore=false` (Interactable, can click mute buttons).

## 4. Frontend Implementation

### 4.1. Overlay Entry Point
*   Create `client/overlay.html`.
*   Create `client/src/overlay.tsx`.
*   This is a separate React/Solid root. It connects to the same local WebSocket or shares state via LocalStorage/Tauri Events?
    *   *Decision:* Tauri Windows share the same Rust backend context. The Overlay frontend will act as a secondary client, listening to Tauri Events emitted by the Main Window or Rust Core.

## 5. Step-by-Step Plan
1.  **Tray:** Implement `SystemTray` in `main.rs`.
2.  **Window:** Add "overlay" window to `tauri.conf.json`.
3.  **Rust:** Implement `set_overlay_input_passthrough` command (Windows specific first).
4.  **Client:** Build `overlay.tsx` UI (Compact User List).
5.  **Sync:** Ensure Overlay receives state updates (Voice Participants) from the Main Window via `emit("voice-state-update")`.