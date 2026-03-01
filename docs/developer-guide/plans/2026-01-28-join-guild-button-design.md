# Join Guild Button Design

## Overview

Add a "Join Guild" button to the server rail that opens a modal where users can paste either a bare invite code (`aBcD1234`) or a full invite URL (`https://example.com/invite/aBcD1234`).

## Design

### JoinGuildModal Component

- Follows `CreateGuildModal` pattern (Portal, fixed overlay, 480px width, header/content/footer)
- Single text input accepting both bare invite codes and full URLs
- Code extraction logic:
  - Try URL match: `/invite/([A-Za-z0-9]+)$/`
  - Try bare code match: `/^[A-Za-z0-9]{8,16}$/`
  - Show validation error if neither matches
- On success: close modal, navigate to joined guild
- Uses existing `joinViaInviteCode(code)` from guilds store

### ServerRail Integration

- Add join button next to existing "+" (Create Guild) button
- Use `UserPlus` icon from lucide-solid
- Toggle between CreateGuildModal and JoinGuildModal

### No Server Changes

The existing `POST /api/invites/{code}/join` endpoint and `joinViaInviteCode()` store function handle everything needed.

## Files to Create/Modify

1. **Create** `client/src/components/guilds/JoinGuildModal.tsx`
2. **Modify** `client/src/components/layout/ServerRail.tsx` — add join button + modal toggle
