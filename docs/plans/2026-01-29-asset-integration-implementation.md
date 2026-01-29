# Asset Integration — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate the generated Finnish Lapphund mascot and branding assets into the application — favicon, login/register hero logo, empty channel state illustration, and Tauri app icons. Theme-aware: standard assets for most themes, pixel variants for Pixel Cozy.

**Architecture:** Copy generated assets from the Gemini artifact directory into `client/public/assets/` with a clean directory structure. Update Login, Register, MessageList empty state, index.html favicon, and Tauri icon config. Use `getThemeFamily()` from the theme store to pick standard vs pixel assets at runtime.

**Tech Stack:** Static PNG assets, Solid.js reactive theme switching, Tauri icon configuration, HTML meta tags.

---

## Context

### Asset Source

All generated assets are at:
```
/home/detair/.gemini/antigravity/brain/e405dfe9-b997-4d83-a4a9-ce56d2846159/
```

Refer to the **Asset Integration Manual** at the same path (`asset_integration_manual.md`) for the original design intent.

### Generated Assets Available

| Artifact File | Size | Purpose |
|--------------|------|---------|
| `logo_hero_png_1769642804729.png` | 567KB | Hero logo for splash/login (1024x1024) |
| `logo_icon_png_1769642823634.png` | 562KB | App icon / launcher (1024x1024) |
| `logo_monochrome_png_1769642843899.png` | 742KB | Watermarks / overlays (1024x1024) |
| `empty_channel_png_1769642868930.png` | 468KB | Empty message list illustration |
| `session_game_timer_png_1769642892367.png` | 500KB | Live Session gaming icon (Phase 6) |
| `session_work_notepad_png_1769642912289.png` | 518KB | Live Session work icon (Phase 6) |
| `session_raid_roles_png_1769642926000.png` | 509KB | Live Session raid icon (Phase 6) |
| `pixel_logo_hero_png_1769643603121.png` | 533KB | Pixel Cozy hero logo |
| `pixel_logo_icon_png_1769643624180.png` | 490KB | Pixel Cozy app icon |
| `pixel_empty_channel_png_1769643644650.png` | 525KB | Pixel Cozy empty state |
| `pixel_session_timer_png_1769643665618.png` | 670KB | Pixel Cozy timer icon |

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| `theme()` | `client/src/stores/theme.ts:95` | Returns current theme ID |
| `getThemeFamily()` | `client/src/stores/theme.ts:148` | Returns `"standard"` or `"pixel"` for a theme ID |
| Theme logos | `client/public/themes/{theme}/logo.png` | Per-theme logo already used somewhere (added Jan 28) |
| Login view | `client/src/views/Login.tsx` | Text-only "Welcome back!" with VoiceChat branding |
| Register view | `client/src/views/Register.tsx` | Text-only "Create an account" |
| MessageList empty state | `client/src/components/messages/MessageList.tsx:169-180` | Uses emoji placeholder `👋` |
| Favicon | `client/index.html:5` | Uses Vite default `/vite.svg` |
| Tauri icons | `client/src-tauri/icons/` | Placeholder 70-byte PNGs, empty ICO/ICNS |
| Tauri config | `client/src-tauri/tauri.conf.json:33-39` | References icon paths |

### What's Missing

1. **No `/assets/` directory structure** in `client/public/`
2. **No logo on Login/Register** — just text headers
3. **Empty state uses emoji** — not the generated mascot illustration
4. **Favicon is Vite default** — `vite.svg` placeholder
5. **Tauri icons are placeholders** — empty/tiny files
6. **App title is "VoiceChat"** — should be "Canis"
7. **No theme-aware asset helper** — each component must manually check theme

---

## Files to Modify

### Assets (copy)
| Source | Target |
|--------|--------|
| `logo_hero_png_*.png` | `client/public/assets/branding/logo-hero.png` |
| `logo_icon_png_*.png` | `client/public/assets/branding/logo-icon.png` |
| `logo_monochrome_png_*.png` | `client/public/assets/branding/logo-monochrome.png` |
| `empty_channel_png_*.png` | `client/public/assets/ui/empty-channel.png` |
| `session_game_timer_png_*.png` | `client/public/assets/toolkits/game-timer.png` |
| `session_work_notepad_png_*.png` | `client/public/assets/toolkits/work-notepad.png` |
| `session_raid_roles_png_*.png` | `client/public/assets/toolkits/raid-roles.png` |
| `pixel_logo_hero_png_*.png` | `client/public/assets/branding/pixel/logo-hero.png` |
| `pixel_logo_icon_png_*.png` | `client/public/assets/branding/pixel/logo-icon.png` |
| `pixel_empty_channel_png_*.png` | `client/public/assets/ui/pixel/empty-channel.png` |
| `pixel_session_timer_png_*.png` | `client/public/assets/toolkits/pixel/game-timer.png` |

### Client
| File | Changes |
|------|---------|
| `client/src/lib/assets.ts` | **NEW** — Theme-aware asset path helper |
| `client/src/views/Login.tsx` | Add hero logo above heading |
| `client/src/views/Register.tsx` | Add hero logo above heading |
| `client/src/components/messages/MessageList.tsx` | Replace emoji with empty-channel illustration |
| `client/index.html` | Update favicon and title to "Canis" |
| `client/src-tauri/tauri.conf.json` | Update product name to "Canis" |

### Tauri Icons
| File | Changes |
|------|---------|
| `client/src-tauri/icons/32x32.png` | Generate from logo-icon.png |
| `client/src-tauri/icons/128x128.png` | Generate from logo-icon.png |
| `client/src-tauri/icons/128x128@2x.png` | Generate from logo-icon.png |
| `client/src-tauri/icons/icon.ico` | Generate from logo-icon.png |
| `client/src-tauri/icons/icon.icns` | Generate from logo-icon.png |

---

## Implementation Tasks

### Task 1: Copy and Organize Assets

**Purpose:** Move all generated assets from the Gemini artifact directory into `client/public/assets/` with clean names.

**Step 1: Create directory structure**

```bash
mkdir -p client/public/assets/branding/pixel
mkdir -p client/public/assets/ui/pixel
mkdir -p client/public/assets/toolkits/pixel
```

**Step 2: Copy and rename standard assets**

```bash
GEMINI_DIR="/home/detair/.gemini/antigravity/brain/e405dfe9-b997-4d83-a4a9-ce56d2846159"

cp "$GEMINI_DIR/logo_hero_png_1769642804729.png" client/public/assets/branding/logo-hero.png
cp "$GEMINI_DIR/logo_icon_png_1769642823634.png" client/public/assets/branding/logo-icon.png
cp "$GEMINI_DIR/logo_monochrome_png_1769642843899.png" client/public/assets/branding/logo-monochrome.png
cp "$GEMINI_DIR/empty_channel_png_1769642868930.png" client/public/assets/ui/empty-channel.png
cp "$GEMINI_DIR/session_game_timer_png_1769642892367.png" client/public/assets/toolkits/game-timer.png
cp "$GEMINI_DIR/session_work_notepad_png_1769642912289.png" client/public/assets/toolkits/work-notepad.png
cp "$GEMINI_DIR/session_raid_roles_png_1769642926000.png" client/public/assets/toolkits/raid-roles.png
```

**Step 3: Copy and rename pixel assets**

```bash
cp "$GEMINI_DIR/pixel_logo_hero_png_1769643603121.png" client/public/assets/branding/pixel/logo-hero.png
cp "$GEMINI_DIR/pixel_logo_icon_png_1769643624180.png" client/public/assets/branding/pixel/logo-icon.png
cp "$GEMINI_DIR/pixel_empty_channel_png_1769643644650.png" client/public/assets/ui/pixel/empty-channel.png
cp "$GEMINI_DIR/pixel_session_timer_png_1769643665618.png" client/public/assets/toolkits/pixel/game-timer.png
```

**Step 4: Verify all files copied correctly**

```bash
find client/public/assets -type f | sort
```

Expected output:
```
client/public/assets/branding/logo-hero.png
client/public/assets/branding/logo-icon.png
client/public/assets/branding/logo-monochrome.png
client/public/assets/branding/pixel/logo-hero.png
client/public/assets/branding/pixel/logo-icon.png
client/public/assets/toolkits/game-timer.png
client/public/assets/toolkits/pixel/game-timer.png
client/public/assets/toolkits/raid-roles.png
client/public/assets/toolkits/work-notepad.png
client/public/assets/ui/empty-channel.png
client/public/assets/ui/pixel/empty-channel.png
```

**Step 5: Commit**

```bash
git add client/public/assets/
git commit -m "chore: add branding assets (hero, icon, monochrome, empty state, toolkit icons)"
```

---

### Task 2: Theme-Aware Asset Helper

**Files:**
- Create: `client/src/lib/assets.ts`

**Purpose:** Centralized helper that returns the correct asset path based on the current theme family. Avoids duplicating theme-check logic in every component.

```typescript
/**
 * Theme-aware asset path resolver.
 * Returns the pixel variant path when the active theme is in the "pixel" family,
 * otherwise returns the standard path.
 */
import { theme, getThemeFamily } from "@/stores/theme";

/**
 * Get the correct asset path based on the current theme family.
 * For "pixel" family themes, returns the pixel variant if it exists.
 *
 * @param standardPath - Path relative to /assets/ (e.g. "branding/logo-hero.png")
 * @param pixelPath - Optional pixel variant path. If omitted, prefixes the
 *   directory with "pixel/" (e.g. "branding/pixel/logo-hero.png").
 */
export function themedAsset(standardPath: string, pixelPath?: string): string {
  const family = getThemeFamily(theme());

  if (family === "pixel") {
    if (pixelPath) return `/assets/${pixelPath}`;
    // Auto-derive pixel path: "branding/logo-hero.png" -> "branding/pixel/logo-hero.png"
    const lastSlash = standardPath.lastIndexOf("/");
    if (lastSlash >= 0) {
      return `/assets/${standardPath.slice(0, lastSlash)}/pixel/${standardPath.slice(lastSlash + 1)}`;
    }
    return `/assets/pixel/${standardPath}`;
  }

  return `/assets/${standardPath}`;
}

/** Convenience: hero logo path for current theme */
export const heroLogo = () => themedAsset("branding/logo-hero.png");

/** Convenience: icon logo path for current theme */
export const iconLogo = () => themedAsset("branding/logo-icon.png");

/** Convenience: empty channel illustration for current theme */
export const emptyChannelArt = () => themedAsset("ui/empty-channel.png");
```

**Design decisions:**
- Returns absolute paths starting with `/assets/` (served from `client/public/assets/`)
- Auto-derives pixel paths by inserting `/pixel/` before the filename — matches the directory structure from Task 1
- Convenience functions are reactive (call `theme()` inside, so Solid tracks changes)
- Only standard and pixel variants exist today. If more families are added later, extend the function

**Verification:**
```bash
cd client && bun run check
```

---

### Task 3: Update Login View with Hero Logo

**Files:**
- Modify: `client/src/views/Login.tsx`

**Step 1: Add import**

```typescript
import { heroLogo } from "@/lib/assets";
```

**Step 2: Add logo above the heading**

Insert before the `<h1>` tag (line 46):

```tsx
<img
  src={heroLogo()}
  alt="Canis"
  class="w-32 h-32 mx-auto mb-4 rounded-2xl object-cover"
/>
```

**Step 3: Update branding text**

Change line 50:
```tsx
// Before:
<p class="text-text-secondary text-center mb-6">
  Login to continue to VoiceChat
</p>

// After:
<p class="text-text-secondary text-center mb-6">
  Login to continue to Canis
</p>
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 4: Update Register View with Hero Logo

**Files:**
- Modify: `client/src/views/Register.tsx`

**Step 1: Add import**

```typescript
import { heroLogo } from "@/lib/assets";
```

**Step 2: Add logo above the heading**

Insert before the heading text, same pattern as Login:

```tsx
<img
  src={heroLogo()}
  alt="Canis"
  class="w-32 h-32 mx-auto mb-4 rounded-2xl object-cover"
/>
```

**Step 3: Update branding text**

Replace any "VoiceChat" references with "Canis".

**Verification:**
```bash
cd client && bun run check
```

---

### Task 5: Update MessageList Empty State

**Files:**
- Modify: `client/src/components/messages/MessageList.tsx`

**Step 1: Add import**

```typescript
import { emptyChannelArt } from "@/lib/assets";
```

**Step 2: Replace emoji placeholder**

Replace lines 170-173 (the empty state visual):

```tsx
// Before:
<div class="w-20 h-20 bg-surface-layer2 rounded-full flex items-center justify-center mb-4">
  <span class="text-4xl">👋</span>
</div>

// After:
<img
  src={emptyChannelArt()}
  alt=""
  class="w-48 h-48 mb-4 opacity-40 select-none pointer-events-none object-contain"
/>
```

**Step 3: Optionally update the text**

The existing text "No messages yet" and "Be the first to send a message in this channel!" is fine. Optionally soften to match the Asset Integration Manual's suggestion:

```tsx
<h3 class="text-lg font-semibold text-text-primary mb-2">
  It's a bit quiet here...
</h3>
<p class="text-text-secondary max-w-sm">
  Be the first to send a message!
</p>
```

This is a minor copy change — the implementer can keep the original text if preferred.

**Verification:**
```bash
cd client && bun run check
```

---

### Task 6: Update Favicon and HTML Metadata

**Files:**
- Modify: `client/index.html`

**Step 1: Generate a favicon from the icon asset**

The logo-icon is 1024x1024. For a favicon, we need a smaller PNG (32x32 or 64x64). The simplest approach is to reference the full icon and let the browser scale it:

```bash
# Copy the icon as favicon
cp client/public/assets/branding/logo-icon.png client/public/favicon.png
```

**Step 2: Update index.html**

```html
<!-- Before: -->
<link rel="icon" type="image/svg+xml" href="/vite.svg" />
<title>VoiceChat</title>

<!-- After: -->
<link rel="icon" type="image/png" href="/favicon.png" />
<title>Canis</title>
```

Also update the meta description:

```html
<!-- Before: -->
<meta name="description" content="VoiceChat - Self-hosted voice and text communication platform" />

<!-- After: -->
<meta name="description" content="Canis - Self-hosted voice and text communication platform" />
```

**Step 3: Remove the old Vite SVG**

```bash
rm client/public/vite.svg 2>/dev/null || true
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 7: Generate Tauri App Icons

**Files:**
- Modify: `client/src-tauri/icons/32x32.png`
- Modify: `client/src-tauri/icons/128x128.png`
- Modify: `client/src-tauri/icons/128x128@2x.png`
- Modify: `client/src-tauri/icons/icon.ico`
- Modify: `client/src-tauri/icons/icon.icns`
- Modify: `client/src-tauri/tauri.conf.json`

**Step 1: Check for image conversion tools**

```bash
which convert 2>/dev/null || which magick 2>/dev/null || which ffmpeg 2>/dev/null
```

If ImageMagick is available (`convert` or `magick`):

```bash
ICON_SRC="client/public/assets/branding/logo-icon.png"

# Generate PNG sizes
convert "$ICON_SRC" -resize 32x32 client/src-tauri/icons/32x32.png
convert "$ICON_SRC" -resize 128x128 client/src-tauri/icons/128x128.png
convert "$ICON_SRC" -resize 256x256 client/src-tauri/icons/128x128@2x.png

# Generate ICO (multi-size)
convert "$ICON_SRC" -resize 256x256 \
  \( -clone 0 -resize 16x16 \) \
  \( -clone 0 -resize 32x32 \) \
  \( -clone 0 -resize 48x48 \) \
  \( -clone 0 -resize 64x64 \) \
  \( -clone 0 -resize 128x128 \) \
  \( -clone 0 -resize 256x256 \) \
  -delete 0 client/src-tauri/icons/icon.ico

# Generate ICNS (macOS) - if png2icns available, otherwise skip
which png2icns && png2icns client/src-tauri/icons/icon.icns \
  <(convert "$ICON_SRC" -resize 16x16 png:-) \
  <(convert "$ICON_SRC" -resize 32x32 png:-) \
  <(convert "$ICON_SRC" -resize 128x128 png:-) \
  <(convert "$ICON_SRC" -resize 256x256 png:-) \
  <(convert "$ICON_SRC" -resize 512x512 png:-)
```

If ImageMagick is NOT available, the Tauri CLI can generate icons:

```bash
cd client && npx tauri icon ../client/public/assets/branding/logo-icon.png
```

**Alternative:** If no image tool is available, at minimum copy the 1024x1024 PNG to the icon files. Browsers and most OSes will scale them. This is not ideal but functional:

```bash
cp client/public/assets/branding/logo-icon.png client/src-tauri/icons/32x32.png
cp client/public/assets/branding/logo-icon.png client/src-tauri/icons/128x128.png
cp client/public/assets/branding/logo-icon.png client/src-tauri/icons/128x128@2x.png
```

The ICO and ICNS files require proper conversion tools. If unavailable, leave them empty and note it as a follow-up.

**Step 2: Update Tauri config product name**

In `client/src-tauri/tauri.conf.json`, change:

```json
// Before:
"productName": "VoiceChat",

// After:
"productName": "Canis",
```

And the window title:

```json
// Before:
"title": "VoiceChat",

// After:
"title": "Canis",
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 8: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

Add under `### Changed` in the `[Unreleased]` section:

```markdown
- App branding updated from "VoiceChat" to "Canis" across all surfaces
  - Finnish Lapphund mascot hero logo on Login and Register screens
  - Theme-aware asset loading (standard and pixel variants)
  - Custom empty channel illustration replaces emoji placeholder
  - Proper favicon replacing Vite default
  - Tauri app icons generated from mascot branding
  - Product name updated in Tauri config, HTML title, and meta tags
```

**Verification:**
```bash
cd client && bun run check
```

---

## Verification

### Client
```bash
cd client && bun run check
```

### Manual Testing

**Login screen:**
1. Navigate to `/login` — verify mascot hero logo appears above "Welcome back!"
2. Switch to Pixel Cozy theme (if accessible from login) — verify pixel variant loads
3. Verify "Canis" branding text (not "VoiceChat")

**Register screen:**
1. Navigate to `/register` — verify same hero logo appears
2. Verify "Canis" branding text

**Empty channel state:**
1. Open a channel with no messages
2. Verify mascot illustration appears instead of 👋 emoji
3. Switch themes — verify standard vs pixel illustration swaps reactively

**Favicon:**
1. Check browser tab — verify Canis icon (not Vite logo)
2. Check page title shows "Canis"

**Tauri app (if available):**
1. Build Tauri app — verify window title is "Canis"
2. Verify taskbar/dock icon uses mascot branding

**Theme switching:**
1. Change to Pixel Cozy theme
2. Navigate to Login — verify pixel hero logo
3. Open empty channel — verify pixel empty state
4. Change back to standard theme — verify standard assets restore

---

## Scope Exclusions

The following are intentionally NOT included in this plan:

- **Toolkit icons** (`game-timer.png`, `work-notepad.png`, `raid-roles.png`) — These are for the Phase 6 Live Session Toolkits feature. The assets are copied into place (Task 1) but not wired into any UI. They're ready for when Phase 6 is implemented.
- **Monochrome logo** (`logo-monochrome.png`) — Copied into place but no current UI uses watermarks or overlays. Available for future use.
- **ServerRail Home icon** — Currently uses Lucide `<Home>` icon which is clean and functional. Replacing it with the mascot icon is a design choice that should be validated visually first. The asset is available at `/assets/branding/logo-icon.png` if desired later.
- **Splash screen / About page** — Phase 6 scope.
