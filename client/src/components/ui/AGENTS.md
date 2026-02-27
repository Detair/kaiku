# UI Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Primitive, reusable UI components. Design system building blocks used throughout the application. Dumb components with no business logic.

## Key Files

### Avatar.tsx

User avatar component with fallback initials and status indicator.

**Props:**

- `src?: string | null` - Avatar image URL
- `alt: string` - User's display name (for initials fallback)
- `size?: "sm" | "md" | "lg"` - Size variant (default: "md")
- `status?: UserStatus` - Online status for indicator
- `showStatus?: boolean` - Display status indicator

**Sizes:**

- `sm` - 32px (w-8 h-8, text-xs)
- `md` - 40px (w-10 h-10, text-sm)
- `lg` - 48px (w-12 h-12, text-base)

**Fallback Behavior:**

- No image → Show initials on colored background
- Initials: First 2 chars or first char of each word
- Color: Generated from name hash (16 color palette)

**Status Indicator:**

- Positioned bottom-right of avatar
- Uses StatusIndicator component
- Responsive to avatar size

**Usage:**

```tsx
<Avatar
  src={user.avatar_url}
  alt={user.display_name}
  size="md"
  status="online"
  showStatus={true}
/>
```

### StatusIndicator.tsx

Online status dot indicator.

**Expected Props:**

- `status: UserStatus` - "online" | "idle" | "dnd" | "invisible" | "offline"
- `size?: "sm" | "md" | "lg"` - Match avatar size

**Visual Design:**

- Absolute positioned (for avatar overlay)
- Colored dot with border
- Colors: green (online), yellow (idle), red (dnd), gray (invisible/offline)

### CodeBlock.tsx

Syntax-highlighted code block for messages.

**Features:**

- Language detection from fence (```lang)
- Syntax highlighting (likely using highlight.js or prism.js)
- Line numbers
- Copy button

**Props:**

- `code: string` - Code content
- `language?: string` - Language for highlighting

**Usage:**

```tsx
// In markdown renderer
<CodeBlock code={codeString} language="typescript" />
```

## Expected UI Components

### Button.tsx

**Variants:**

- Primary (accent color)
- Secondary (subtle)
- Danger (red)
- Ghost (transparent)

**Sizes:** sm, md, lg

**States:** normal, hover, active, disabled, loading

### Input.tsx

**Types:**

- text, password, email, number, search

**Features:**

- Label support
- Error state
- Helper text
- Icons (left/right)

### Select.tsx

Dropdown select component.

### Checkbox.tsx

Checkbox with label.

### Radio.tsx

Radio button with label.

### Switch.tsx

Toggle switch component.

### Tooltip.tsx

Hover tooltip.

**Props:**

- `content: string` - Tooltip text
- `placement?: "top" | "bottom" | "left" | "right"`

### Modal.tsx

Base modal wrapper.

**Features:**

- Portal rendering
- Backdrop blur
- ESC to close
- Click outside to close
- Animation (fade + scale)

### Spinner.tsx

Loading spinner.

**Sizes:** sm, md, lg

### Badge.tsx

Small colored badge (for counts, status).

### Divider.tsx

Horizontal or vertical divider line.

### Skeleton.tsx

Loading placeholder (shimmer effect).

## Design Tokens

### Colors

From CSS custom properties:

- `--color-accent-primary` - Brand color
- `--color-text-primary` - Main text
- `--color-text-secondary` - Subtle text
- `--color-surface-base` - Base background
- `--color-surface-layer1` - Elevated background
- `--color-surface-layer2` - More elevated
- `--color-error-text` - Error messages
- `--color-success-text` - Success messages

### Spacing

Tailwind scale: 0.5, 1, 1.5, 2, 2.5, 3, 4, 5, 6, 8, 10, 12, 16, 20, 24

### Border Radius

- `rounded-lg` - Default (8px)
- `rounded-xl` - Large (12px)
- `rounded-2xl` - XLarge (16px)
- `rounded-full` - Circle

### Typography

- Font: Inter (from Google Fonts or bundled)
- Sizes: text-xs, text-sm, text-base, text-lg, text-xl, text-2xl
- Weights: font-normal (400), font-medium (500), font-semibold (600), font-bold (700)

## Component Patterns

### Composition Over Configuration

Prefer composable components over monolithic props:

```tsx
// Good
<Modal>
  <ModalHeader>Title</ModalHeader>
  <ModalBody>Content</ModalBody>
  <ModalFooter>Actions</ModalFooter>
</Modal>

// Avoid
<Modal title="Title" content="Content" footer={actions} />
```

### Controlled vs Uncontrolled

- Form inputs: Support both controlled and uncontrolled
- Modals: Parent controls visibility

### Accessibility

- Semantic HTML (button, input, etc.)
- ARIA labels where needed
- Keyboard navigation
- Focus management
- Screen reader support

## Styling Approach

### UnoCSS Utility-First

Components use Tailwind-compatible utilities via UnoCSS:

```tsx
<button class="px-4 py-2 bg-accent-primary text-white rounded-lg hover:opacity-90">
  Click me
</button>
```

### Dynamic Styles

```tsx
// Use classList for conditional classes
<div
  class="base-class"
  classList={{
    "active-class": isActive(),
    "disabled-class": isDisabled(),
  }}
>
```

### CSS Variables for Theming

```tsx
<div style="background-color: var(--color-surface-base)">
```

## Testing Considerations

UI components should:

- Render with default props
- Handle all prop combinations
- Show correct states (hover, active, disabled)
- Be keyboard accessible
- Have snapshot tests for visual regression

## Future Enhancements

- Storybook for component documentation
- Automated visual regression tests
- Component prop type generation (TypeScript)
- Dark mode variants
- Animation system (Framer Motion or Solid Transition Group)

## Related Documentation

- Design system: `docs/design-system.md` (if exists)
- UnoCSS config: `client/uno.config.ts`
- Theme tokens: `client/src/styles/design-tokens.css`
- Accessibility guidelines: `docs/accessibility.md` (if exists)
