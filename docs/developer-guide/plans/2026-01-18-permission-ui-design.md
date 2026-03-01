# Permission System UI Design

## Overview

UI components for guild role management, channel permission overrides, and system admin dashboard. Builds on the existing permission store and API layer.

**Date:** 2026-01-18
**Status:** Design Complete
**Batch:** 3 (UI Components)

---

## Scope

### In Scope (This Batch)
- Roles Tab in GuildSettingsModal
- Role Editor with permission checkboxes
- Members Tab enhancement (role badges, assignment dropdown)
- Channel Settings Modal with Permissions tab
- Admin Quick Modal (elevation, quick stats, link to dashboard)
- Admin Dashboard page (users, guilds, audit log, actions)

### Future Enhancements
- Metrics/Monitoring page (server metrics, voice latency, jitter)
- Break-glass emergency flow UI
- System announcements management
- System settings configuration

---

## Component Designs

### 1. Roles Tab

**Location:** New tab in `GuildSettingsModal.tsx`

```
┌─────────────────────────────────────────────────────────┐
│  [Invites]  [Members]  [Roles]                          │
├─────────────────────────────────────────────────────────┤
│  Roles                                    [+ New Role]  │
│  ┌───────────────────────────────────────────────────┐  │
│  │ ● Officer                              [Edit] [⋮] │  │
│  │   8 permissions • 2 members                       │  │
│  ├───────────────────────────────────────────────────┤  │
│  │ ● Moderator                            [Edit] [⋮] │  │
│  │   5 permissions • 4 members                       │  │
│  ├───────────────────────────────────────────────────┤  │
│  │ ○ @everyone                            [Edit]     │  │
│  │   Base permissions for all members                │  │
│  └───────────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────┘
```

**Behaviors:**
- Roles sorted by position (highest rank first)
- Color dot shows role color (○ for no color)
- `@everyone` cannot be deleted (no ⋮ menu)
- [Edit] opens a slide-out panel for permission editing
- [⋮] menu: "Manage Members", "Delete Role"
- Only shown to users with `MANAGE_ROLES` permission

---

### 2. Role Editor Panel

Slide-out panel within the modal:

```
┌─────────────────────────────────────────────────────────┐
│  ← Back                           Edit Role: Moderator  │
├─────────────────────────────────────────────────────────┤
│  Role Name                                              │
│  ┌───────────────────────────────────────────────────┐  │
│  │ Moderator                                         │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  Color   [● #3498db ▼]                                  │
├─────────────────────────────────────────────────────────┤
│  PERMISSIONS                                            │
│                                                         │
│  Content                                                │
│  ┌─ ☑ Send Messages                                     │
│  │    Allows sending text messages in channels          │
│  ├─ ☑ Embed Links                                       │
│  └─ ☑ Attach Files                                      │
│                                                         │
│  Moderation                                             │
│  ┌─ ☑ Manage Messages                                   │
│  │    Allows deleting messages from other members       │
│  ├─ ☑ Timeout Members                                   │
│  └─ ☐ Kick Members                                      │
│     ⚠ You don't have this permission                    │
│                                                         │
├─────────────────────────────────────────────────────────┤
│  Members with this role (4)              [+ Add Member] │
│  ┌───────────────────────────────────────────────────┐  │
│  │ [Avatar] alice          [@alice_wonder]    [×]    │  │
│  │ [Avatar] bob            [@bob_builder]     [×]    │  │
│  └───────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────┤
│                              [Cancel]  [Save Changes]   │
└─────────────────────────────────────────────────────────┘
```

**Key behaviors:**
- Permissions grouped by category (from `permissionConstants.ts`)
- Disabled checkboxes for permissions the editor doesn't have (escalation prevention)
- Warning text explains why a permission is disabled
- `@everyone` editor hides forbidden permissions entirely
- **Security:** API explicitly forbids granting dangerous permissions (e.g. `MANAGE_GUILD`, `BAN_MEMBERS`) to `@everyone` even if UI checks are bypassed
- [+ Add Member] opens a member picker dropdown
- Unsaved changes trigger confirmation on Back/Cancel

---

### 3. Members Tab Enhancement

Extends existing Members Tab to show and manage roles:

```
┌─────────────────────────────────────────────────────────┐
│ 🔍 Search members...                                    │
├─────────────────────────────────────────────────────────┤
│ 12 Members                                              │
│ ┌─────────────────────────────────────────────────────┐ │
│ │ [Avatar] detair              👑 Owner               │ │
│ │          @detair                                    │ │
│ │          Joined Jan 14 • Online                  🟢 │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ [Avatar] alice                                      │ │
│ │          @alice_wonder                              │ │
│ │          ● Officer  ● Moderator         [Manage ▼] │ │
│ │          Joined Jan 14 • 2 hours ago             ⚫ │ │
│ ├─────────────────────────────────────────────────────┤ │
│ │ [Avatar] bob                                        │ │
│ │          @bob_builder                               │ │
│ │          (no roles)                     [Manage ▼] │ │
│ │          Joined Jan 15 • Online                  🟢 │ │
│ └─────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

**[Manage ▼] dropdown:**
```
┌────────────────────────┐
│ Assign Role            │
│ ├─ ☐ Officer           │
│ ├─ ☑ Moderator         │
│ └─ ☐ VIP               │
├────────────────────────┤
│ Kick from Server       │
└────────────────────────┘
```

**Key behaviors:**
- Role badges shown inline with color dots
- [Manage] only visible to users with `MANAGE_ROLES` or owner
- Roles in dropdown disabled if above user's highest role (hierarchy)
- Checkboxes toggle immediately (optimistic update)
- Owner row never shows [Manage] dropdown
- Existing kick functionality moves into this dropdown

---

### 4. Channel Settings Modal

New modal accessed via right-click channel → "Edit Channel":

```
┌─────────────────────────────────────────────────────────┐
│  # general                                      [X]     │
├─────────────────────────────────────────────────────────┤
│  [Overview]  [Permissions]                              │
├─────────────────────────────────────────────────────────┤
│  PERMISSIONS TAB                                        │
│                                                         │
│  Role Overrides                        [+ Add Role]     │
│  ┌───────────────────────────────────────────────────┐  │
│  │ ● Moderator                          [Edit] [×]   │  │
│  │   ✓ 2 allowed  ✗ 1 denied                         │  │
│  ├───────────────────────────────────────────────────┤  │
│  │ ○ @everyone                          [Edit] [×]   │  │
│  │   ✗ 1 denied (Send Messages)                      │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  No overrides = inherits from role's base permissions   │
└─────────────────────────────────────────────────────────┘
```

**Override Editor:**
```
┌─────────────────────────────────────────────────────────┐
│  Moderator in #general                                  │
├─────────────────────────────────────────────────────────┤
│  Permission          Inherit    Allow    Deny          │
│  ─────────────────────────────────────────────────────  │
│  Send Messages         ◉         ○        ○            │
│  Embed Links           ○         ◉        ○            │
│  Attach Files          ○         ○        ◉            │
│  Manage Messages       ◉         ○        ○            │
├─────────────────────────────────────────────────────────┤
│                                         [Save]          │
└─────────────────────────────────────────────────────────┘
```

**Key behaviors:**
- Three-state radio: Inherit (use role default) / Allow / Deny
- Only `MANAGE_CHANNELS` permission holders can edit
- Overview tab: channel name, topic, type (future use)
- [+ Add Role] shows dropdown of roles without overrides yet

---

### 5. Admin Quick Modal

Accessible from user menu for system admins only:

```
┌─────────────────────────────────────────────────────────┐
│  Admin Panel                                    [X]     │
├─────────────────────────────────────────────────────────┤
│  Session Status                                         │
│  ┌───────────────────────────────────────────────────┐  │
│  │  🔓 Not Elevated                                  │  │
│  │  Destructive actions require elevation            │  │
│  │                           [Elevate Session]       │  │
│  └───────────────────────────────────────────────────┘  │
│                                                         │
│  OR (when elevated):                                    │
│  ┌───────────────────────────────────────────────────┐  │
│  │  🔐 Elevated                    Expires in 12:34  │  │
│  │                              [De-elevate]         │  │
│  └───────────────────────────────────────────────────┘  │
├─────────────────────────────────────────────────────────┤
│  Quick Stats                                            │
│  ┌─────────────┐  ┌─────────────┐  ┌─────────────┐     │
│  │    127      │  │     12      │  │      3      │     │
│  │   Users     │  │   Guilds    │  │   Banned    │     │
│  └─────────────┘  └─────────────┘  └─────────────┘     │
├─────────────────────────────────────────────────────────┤
│  [📋 Open Full Dashboard]                               │
│                                                         │
│  Opens detailed admin view with user management,        │
│  guild oversight, and audit logs.                       │
└─────────────────────────────────────────────────────────┘
```

**Key behaviors:**
- Only rendered if user is system admin
- Shows elevation status with countdown timer
- [Elevate Session] triggers MFA prompt
- Quick stats fetched on modal open
- Link to full dashboard at `/admin`

---

### 6. Admin Dashboard Page (`/admin`)

Full page with sidebar navigation:

```
┌──────────────────────────────────────────────────────────────────────┐
│  ← Back to App                              Admin Dashboard    🔐    │
├────────────┬─────────────────────────────────────────────────────────┤
│            │                                                         │
│  Overview  │  USERS                               🔍 Search users... │
│  ─────────►│  ┌─────────────────────────────────────────────────────┐│
│  Users     │  │ Username      Email           Joined     Status    ││
│            │  ├─────────────────────────────────────────────────────┤│
│  Guilds    │  │ alice         alice@ex.com    Jan 12     Active    ││
│            │  │ bob           bob@ex.com      Jan 14     Active    ││
│  Audit Log │  │ mallory       mal@ex.com      Jan 15     🚫 Banned ││
│            │  └─────────────────────────────────────────────────────┘│
│            │                                                         │
│            │  Selected: alice                                        │
│            │  ┌─────────────────────────────────────────────────────┐│
│            │  │ alice_wonder                                        ││
│            │  │ alice@example.com • Joined Jan 12, 2026             ││
│            │  │ Guilds: Gaming Hub, Dev Team                        ││
│            │  │                                                     ││
│            │  │ [Ban User]  (requires elevation)                    ││
│            │  └─────────────────────────────────────────────────────┘│
│            │                                                         │
│            │  ◀ 1 2 3 ... 10 ▶                                      │
└────────────┴─────────────────────────────────────────────────────────┘
```

**Sections:**
- **Overview**: Quick stats, recent activity, system health
- **Users**: Paginated list, search, detail panel, ban/unban actions
- **Guilds**: Paginated list, member count, suspend/unsuspend actions
- **Audit Log**: Filterable log with actor, action, target, timestamp

**Elevation UX:**
- 🔐 indicator in header shows elevated status
- Action buttons disabled with tooltip "Requires elevation" when not elevated
- Clicking disabled button prompts "Elevate now?"

---

## File Structure

**New files:**

```
client/src/components/
├── guilds/
│   ├── RolesTab.tsx           # Role list in GuildSettingsModal
│   ├── RoleEditor.tsx         # Slide-out permission editor
│   └── MemberRoleDropdown.tsx # Role assignment dropdown for members
├── channels/
│   ├── ChannelSettingsModal.tsx  # New modal with Overview + Permissions
│   └── ChannelPermissions.tsx    # Override editor component
├── admin/
│   ├── AdminQuickModal.tsx    # Quick access modal
│   ├── AdminDashboard.tsx     # Full page at /admin
│   ├── AdminSidebar.tsx       # Navigation sidebar
│   ├── UsersPanel.tsx         # User management
│   ├── GuildsPanel.tsx        # Guild oversight
│   ├── AuditLogPanel.tsx      # Audit log viewer
│   └── ElevationBanner.tsx    # Elevation status component
```

**Modifications:**
- `GuildSettingsModal.tsx` - Add Roles tab
- `MembersTab.tsx` - Add role badges and [Manage] dropdown
- `ChannelItem.tsx` - Add context menu with "Edit Channel"
- `UserPanel.tsx` - Add "Admin Panel" option for admins
- `App.tsx` - Add `/admin` route

---

## Implementation Order

1. **RolesTab + RoleEditor** (most foundational)
2. **MembersTab enhancements** (uses role data)
3. **ChannelSettingsModal + Permissions**
4. **AdminQuickModal + AdminDashboard**

---

## Dependencies

**Existing (already implemented):**
- `permissionConstants.ts` - Permission bits and UI definitions
- `permissions.ts` store - Full state management
- `types.ts` - GuildRole, ChannelOverride types
- `tauri.ts` - All API functions for roles and overrides

**API endpoints required (already implemented in backend):**
- `GET/POST /api/guilds/:id/roles`
- `PATCH/DELETE /api/guilds/:id/roles/:id`
- `POST/DELETE /api/guilds/:id/members/:id/roles/:id`
- `GET/PUT/DELETE /api/channels/:id/overrides/:role_id`
- `GET /api/admin/users`
- `GET /api/admin/guilds`
- `GET /api/admin/audit-log`
- `POST/DELETE /api/admin/elevate`
- `POST/DELETE /api/admin/users/:id/ban`
- `POST/DELETE /api/admin/guilds/:id/suspend`
