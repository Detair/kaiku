# Authentication Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Authentication and route protection components. Manages auth flow, guards protected routes, and provides loading states during auth initialization.

## Key Files

### AuthGuard.tsx

Route guard component that protects authenticated routes.

**Responsibilities:**

- Initialize auth state on mount
- Redirect to login if not authenticated
- Preserve return URL for post-login redirect
- Show loading screen during auth initialization

**Usage:**

```tsx
import AuthGuard from "@/components/auth/AuthGuard";

// Wrap protected routes
<AuthGuard>
  <AppShell>{/* Protected content */}</AppShell>
</AuthGuard>;
```

**State Dependencies:**

- `authState.isInitialized` - Auth system ready
- `isAuthenticated()` - User logged in

**Redirect Logic:**

- Stores `returnUrl` query param for post-login redirect
- Uses `navigate(..., { replace: true })` to avoid back button issues

## Future Components

Expected auth components (not yet implemented):

- `LoginForm.tsx` - Email/password login
- `RegisterForm.tsx` - New account creation
- `MFAPrompt.tsx` - TOTP/WebAuthn verification
- `OAuthButtons.tsx` - SSO/OIDC login

## Integration Points

### Stores

- `@/stores/auth` - Auth state, token management, user info

### Routes

- Redirects to `/login` when unauthenticated
- Supports `?returnUrl=` for post-login navigation

## Security Considerations

- NEVER store tokens in localStorage (handled by Tauri secure storage)
- NEVER render protected content before auth check completes
- Always use `replace: true` for auth redirects
- JWT tokens expire after 15min (server-enforced)

## Styling

Uses design system variables:

- `bg-background-primary` - Loading screen background
- `text-text-secondary` - Loading text
- `border-primary` - Spinner colors

## Related Documentation

- Auth flow: `PROJECT_SPEC.md` § Authentication
- Token handling: `STANDARDS.md` § JWT
- Argon2id password hashing: `STANDARDS.md` § Security
