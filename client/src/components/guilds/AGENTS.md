# Guild Components

**Parent:** [../AGENTS.md](../AGENTS.md)

## Purpose

Guild (server) management components. Handles guild settings, member management, invite generation, and guild creation.

## Key Files

### GuildSettingsModal.tsx

Main guild settings dialog with tabbed interface.

**Tabs:**

- **Invites** (owner-only) - Create and manage invite links
- **Members** - View member list, manage roles (kick/ban for owners)

**Access Control:**

- Non-owners default to Members tab
- Invites tab hidden for non-owners
- Tab visibility via `isGuildOwner(guildId, userId)`

**Modal Behavior:**

- ESC key closes modal
- Backdrop click closes modal
- Uses Portal for proper z-index layering

**Usage:**

```tsx
import GuildSettingsModal from "@/components/guilds/GuildSettingsModal";

<Show when={showSettings()}>
  <GuildSettingsModal
    guildId={guild.id}
    onClose={() => setShowSettings(false)}
  />
</Show>;
```

### InvitesTab.tsx

Invite management panel (owner-only).

**Expected Features:**

- Generate new invite links
- Set expiration (1h, 24h, 7d, never)
- Set max uses (1, 10, 25, unlimited)
- Copy invite link to clipboard
- Revoke active invites
- View invite usage stats

### MembersTab.tsx

Guild member list with management actions.

**Expected Features:**

- List all guild members
- Show member roles/permissions
- Kick/ban members (owner only)
- Change member roles (owner only)
- View member join date

**Props:**

- `guildId` - Guild to show members for
- `isOwner` - Enable management actions

### CreateGuildModal.tsx

New guild creation dialog.

**Expected Fields:**

- Guild name (required)
- Guild icon (optional)
- Initial channels (general text, general voice)

**Behavior:**

- Auto-selects new guild after creation
- Creates default channels
- Sets creator as owner

## Guild Roles

**Owner:**

- Full control over guild
- Manage invites, members, channels
- Cannot be removed/demoted
- Transfer ownership (future)

**Member:**

- Basic channel access
- Can view members
- Cannot manage guild

**Future Roles:**

- Admin - Partial management permissions
- Moderator - Kick/ban only
- Custom roles with granular permissions

## State Management

### From Stores

- `guildsState.guilds` - All guilds user is in
- `guildsState.activeGuildId` - Currently selected guild
- `isGuildOwner(guildId, userId)` - Permission check
- `authState.user.id` - Current user ID

## Integration Points

### Stores

- `@/stores/guilds` - Guild data and selection
- `@/stores/auth` - User info for permission checks

### Backend APIs

- `POST /guilds` - Create guild
- `GET /guilds/:id/invites` - List invites
- `POST /guilds/:id/invites` - Create invite
- `DELETE /guilds/:id/invites/:code` - Revoke invite
- `GET /guilds/:id/members` - List members
- `DELETE /guilds/:id/members/:userId` - Kick member

## Styling

**Modal Structure:**

- 600px width, max 80vh height
- Border: `border-white/10`
- Background: `--color-surface-base`
- Rounded corners: `rounded-2xl`

**Tab Styles:**

- Active: `text-accent-primary border-b-2 border-accent-primary`
- Inactive: `text-text-secondary hover:text-text-primary`

**Guild Icon:**

- Fallback to first letter of guild name
- Accent color background
- Rounded corners: `rounded-xl`

## UX Patterns

### Permission-Based UI

- Invites tab hidden for non-owners
- Members tab shows different actions based on `isOwner` prop
- Management buttons disabled/hidden for members

### Modal Navigation

- Tabs persist state during modal session
- Default tab based on user role

### Guild Icon Display

- Initials fallback for guilds without icons
- Consistent with Avatar component pattern

## Future Enhancements

- Guild banner/splash
- Role management UI
- Audit log viewer
- Guild analytics (member growth, activity)
- Guild templates
- Guild discovery settings

## Related Documentation

- Guild permissions: `PROJECT_SPEC.md` § Guilds
- Invite system: `ARCHITECTURE.md` § Invite Service
- Role-based access control: `docs/rbac.md` (if exists)
