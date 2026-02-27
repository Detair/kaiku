# Settings Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

User preferences and application settings. Tabbed interface for appearance, audio, voice, and account settings.

## Key Files

### SettingsModal.tsx

Main settings dialog with sidebar navigation.

**Tab Structure:**

- **Appearance** - Theme, font size, compact mode
- **Audio** - Output device, volume (not yet implemented)
- **Voice** - Input device, noise suppression (not yet implemented)

**Modal Behavior:**

- ESC key closes
- Backdrop click closes
- Portal rendering for proper z-index
- Smooth fade-in animation (0.15s)

**Layout:**

- 700px width, max 600px height
- Left sidebar (192px) with tab buttons
- Right content area (flex-1, scrollable)

**Usage:**

```tsx
import SettingsModal from "@/components/settings/SettingsModal";

<Show when={showSettings()}>
  <SettingsModal onClose={() => setShowSettings(false)} />
</Show>;
```

### AppearanceSettings.tsx

Theme and visual customization.

**Expected Settings:**

- Theme selector (Auto, Light, Dark)
- Accent color picker
- Font size (12px - 18px)
- Compact mode toggle
- Message display density
- Accessibility options

**State:**

- Persisted to localStorage
- Applied via CSS custom properties
- Reactive updates (no page reload needed)

### index.ts

Re-exports SettingsModal for cleaner imports.

## Settings Categories

### Appearance

**Implemented:**

- Theme selection (auto/light/dark)
- Accent color customization
- Font size adjustment

**Future:**

- Custom themes
- Background image
- Saturation control
- High contrast mode
- Reduce motion

### Audio

**Planned:**

- Output device selection
- Output volume slider
- Sound effects toggle
- Notification sounds
- PTT (Push-to-Talk) keybind

### Voice

**Planned:**

- Input device selection
- Input sensitivity slider
- Noise suppression toggle
- Echo cancellation
- Voice activity threshold
- Automatic gain control

### Account (Future)

- Change password
- Enable/disable MFA
- Email address
- Username change
- Avatar upload
- Status message
- Privacy settings

### Notifications (Future)

- Desktop notifications
- Sound settings
- Per-channel overrides
- DND schedule
- Keyword alerts

### Keybinds (Future)

- Mute/unmute
- Deafen
- Push-to-talk
- Command palette
- Custom shortcuts

## State Management

### Persistence

```ts
// localStorage keys
"theme": "auto" | "light" | "dark"
"accentColor": "#hex"
"fontSize": number (12-18)
"compactMode": boolean
"voiceInputDevice": deviceId
"audioOutputDevice": deviceId
```

### Reactive Application

Settings changes immediately update:

- CSS custom properties (`--color-accent-primary`)
- Body class (`theme-dark`, `compact-mode`)
- Audio/voice device selection (Tauri backend)

### Store Integration

Expected settings store structure:

```ts
interface SettingsState {
  theme: Theme;
  accentColor: string;
  fontSize: number;
  compactMode: boolean;
  voiceInput: string;
  audioOutput: string;
  notifications: NotificationSettings;
  keybinds: Keybinds;
}
```

## Integration Points

### Components

- Theme selector component
- Color picker component
- Slider components
- Device selector dropdowns

### Stores

- `@/stores/settings` - Settings state and persistence

### Tauri Backend

- `setAudioDevice(deviceId)` - Change output
- `setVoiceDevice(deviceId)` - Change input
- `getAudioDevices()` - List available devices

## Styling

### Modal Layout

```tsx
<div class="flex">
  {/* Sidebar (192px) */}
  <div class="w-48 border-r border-white/10 p-3">
    <TabButton />
  </div>

  {/* Content (flex-1) */}
  <div class="flex-1 overflow-y-auto p-6">
    <TabContent />
  </div>
</div>
```

### Tab Button States

- **Active:** `bg-accent-primary/20 text-accent-primary`
- **Inactive:** `text-text-secondary hover:text-text-primary hover:bg-white/5`

### Tab Icons

- Palette (Appearance)
- Volume2 (Audio)
- Mic (Voice)

## UX Patterns

### Live Preview

Settings changes apply immediately:

- Theme switches without reload
- Font size updates in real-time
- Accent color propagates instantly

### Default Values

- Theme: "auto" (respects OS preference)
- Accent color: `#5865F2` (default blue)
- Font size: 16px
- Compact mode: false

### Reset Options

Each tab should have "Reset to defaults" button.

## Future Enhancements

### Import/Export Settings

- Export settings as JSON
- Import settings from file
- Share theme presets

### Cloud Sync (Future)

- Sync settings across devices
- Requires backend settings storage
- Privacy-respecting (encrypted)

### Advanced Settings

- Developer mode toggle
- Debug logging
- Hardware acceleration
- Memory limits

### Accessibility

- Screen reader mode
- Keyboard navigation focus indicators
- High contrast themes
- Font weight options
- Color blind modes

## Related Documentation

- Design tokens: `client/src/styles/design-tokens.css`
- Theme system: `docs/theming.md` (if exists)
- Audio settings: `ARCHITECTURE.md` § Audio Pipeline
- Voice processing: `STANDARDS.md` § Opus Codec
