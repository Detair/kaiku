<!-- Parent: ../AGENTS.md -->
# Public Static Assets

## Purpose

Static assets served directly by Vite without processing. Files are copied as-is to the build output.

## Key Files

| File | Purpose |
|------|---------|
| `vite.svg` | Vite logo (default placeholder, replace with app logo) |

## For AI Agents

### What Goes Here

**DO include:**
- Favicon (`favicon.ico`, `favicon.svg`)
- Web manifest (`manifest.json`)
- Robots file (`robots.txt`)
- Static images that don't need processing
- Third-party assets with fixed paths

**DON'T include:**
- Images that need optimization (use `src/assets/`)
- Component-specific assets (co-locate with components)
- Large files (consider CDN)

### Asset URLs

Files in `public/` are available at root URL:
- `public/vite.svg` → `https://app.example.com/vite.svg`
- `public/icons/logo.png` → `https://app.example.com/icons/logo.png`

### Referencing in Code

```tsx
// Direct URL reference
<img src="/vite.svg" alt="Logo" />

// In CSS
.logo {
  background-image: url('/icons/logo.png');
}
```

### Build Behavior

- Files are copied to `dist/` root during build
- No hashing or fingerprinting applied
- Cache headers should be configured at server level

### Current Status

Contains placeholder Vite logo. Replace with Kaiku branding:
- Add `favicon.ico`
- Add `apple-touch-icon.png`
- Add PWA icons if implementing

## Dependencies

- Vite build system
- Served by Tauri WebView or dev server
