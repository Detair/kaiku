<!-- Parent: ../AGENTS.md -->

# pages

## Purpose

Page-level routes and demonstration components. Minimal directory containing special-purpose views that don't fit the main app flow.

## Key Files

- `ThemeDemo.tsx` - Theme showcase page with syntax highlighting examples

## For AI Agents

### Current Usage

This directory is currently minimal:

- `ThemeDemo.tsx` - Developer tool for testing theme system
  - Shows all available themes with live switching
  - Demonstrates CodeBlock component with syntax highlighting
  - Displays color palette reference
  - Used during theme development, not part of main app

### Routing Context

Main app routing handled elsewhere:

- `src/views/` - Primary authenticated views (Main, Login, Register)
- `src/components/` - Reusable components within views
- `src/pages/` - Standalone pages and demos

### Theme Demo Details

Shows three language examples:

- Rust code sample
- TypeScript interface/async function
- Python fibonacci function

Each rendered with `CodeBlock` component using highlight.js:

- Theme-aware syntax colors (CSS variables)
- Language detection
- Line numbering support

### Future Use

This directory could grow to include:

- Settings page
- User profile page
- Server discovery/browse
- Admin panels
- Help/documentation viewer

### Convention

Pages in this directory should:

- Be self-contained (full-screen layouts)
- Not depend on AppShell layout
- Handle their own routing if needed
- Use minimal store dependencies
