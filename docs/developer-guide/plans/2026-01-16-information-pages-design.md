# Information Pages Feature Design

**Date:** 2026-01-16
**Status:** Ready for Implementation
**Priority:** Medium

## Overview

Information pages at platform and guild level for Terms of Service, rules, FAQ, and other documentation. Supports markdown with Mermaid diagrams and image uploads.

## Requirements Summary

| Requirement | Decision |
|-------------|----------|
| Scope | Platform-wide + Per-guild pages |
| UI Location | Guild: collapsible section above channels; Platform: Home view |
| Permissions | Guild: "ManagePages" role permission; Platform: platform admin role |
| Structure | Flat list, max 10 pages per scope |
| Ordering | Manual drag-and-drop (position field) |
| Acceptance | Optional per-page toggle, blocking for platform pages |
| Visibility | Role-based (guild) or all members |
| Rendering | Markdown + Mermaid diagrams + image uploads |
| Editor | Side-by-side with live preview + cheat sheet |
| Deep links | Internal URLs `/guild/:id/pages/:slug` |
| Versioning | Simple edit tracking now, full history later |
| Deletion | Soft delete with future archive option |

---

## Database Schema

### Platform Admin System

```sql
CREATE TABLE platform_roles (
    user_id UUID PRIMARY KEY REFERENCES users(id),
    role VARCHAR(20) NOT NULL DEFAULT 'user',  -- 'user', 'admin'
    granted_by UUID REFERENCES users(id),
    granted_at TIMESTAMPTZ DEFAULT NOW()
);
```

### Pages Table

```sql
CREATE TABLE pages (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
    guild_id UUID REFERENCES guilds(id) ON DELETE CASCADE,

    title VARCHAR(100) NOT NULL,
    slug VARCHAR(100) NOT NULL
        CONSTRAINT slug_format CHECK (slug ~ '^[a-z0-9]([a-z0-9\-]*[a-z0-9])?$'),

    content TEXT NOT NULL,
    content_hash VARCHAR(64) NOT NULL,  -- SHA-256

    position INT NOT NULL DEFAULT 0,
    requires_acceptance BOOLEAN DEFAULT FALSE,

    created_by UUID NOT NULL REFERENCES users(id),
    updated_by UUID NOT NULL REFERENCES users(id),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    deleted_at TIMESTAMPTZ,

    UNIQUE(guild_id, slug) WHERE deleted_at IS NULL
);

CREATE INDEX idx_pages_guild_position ON pages(guild_id, position) WHERE deleted_at IS NULL;
CREATE INDEX idx_pages_platform_position ON pages(position) WHERE guild_id IS NULL AND deleted_at IS NULL;
```

### Audit Log

```sql
CREATE TABLE page_audit_log (
    id UUID PRIMARY KEY DEFAULT uuid_generate_v7(),
    page_id UUID NOT NULL REFERENCES pages(id),
    action VARCHAR(20) NOT NULL,  -- 'create', 'update', 'delete', 'restore'
    actor_id UUID NOT NULL REFERENCES users(id),
    previous_content_hash VARCHAR(64),
    ip_address INET,
    user_agent TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_page_audit_log_page ON page_audit_log(page_id);
```

### User Acceptance Tracking

```sql
CREATE TABLE page_acceptances (
    user_id UUID REFERENCES users(id) ON DELETE CASCADE,
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    content_hash VARCHAR(64) NOT NULL,
    accepted_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (user_id, page_id)
);
```

### Role-Based Visibility

```sql
CREATE TABLE page_visibility (
    page_id UUID REFERENCES pages(id) ON DELETE CASCADE,
    role_id UUID REFERENCES guild_roles(id) ON DELETE CASCADE,
    PRIMARY KEY (page_id, role_id)
);

-- Trigger to enforce role-guild consistency
CREATE OR REPLACE FUNCTION check_page_visibility_guild()
RETURNS TRIGGER AS $$
BEGIN
    IF NOT EXISTS (
        SELECT 1 FROM pages p
        JOIN guild_roles gr ON gr.guild_id = p.guild_id
        WHERE p.id = NEW.page_id AND gr.id = NEW.role_id
    ) THEN
        RAISE EXCEPTION 'Role must belong to same guild as page';
    END IF;
    RETURN NEW;
END;
$$ LANGUAGE plpgsql;

CREATE TRIGGER page_visibility_guild_check
    BEFORE INSERT OR UPDATE ON page_visibility
    FOR EACH ROW EXECUTE FUNCTION check_page_visibility_guild();
```

---

## API Endpoints

### Platform Pages (requires platform admin)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/pages` | Create platform page |
| GET | `/api/pages` | List platform pages |
| GET | `/api/pages/:slug` | Get platform page by slug |
| PATCH | `/api/pages/:id` | Update platform page |
| DELETE | `/api/pages/:id` | Soft delete |
| POST | `/api/pages/:id/restore` | Restore deleted page |
| POST | `/api/pages/reorder` | Update positions |

### Guild Pages (requires ManagePages permission)

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/guilds/:guild_id/pages` | Create guild page |
| GET | `/api/guilds/:guild_id/pages` | List (filtered by role visibility) |
| GET | `/api/guilds/:guild_id/pages/:slug` | Get by slug |
| PATCH | `/api/guilds/:guild_id/pages/:id` | Update |
| DELETE | `/api/guilds/:guild_id/pages/:id` | Soft delete |
| POST | `/api/guilds/:guild_id/pages/:id/restore` | Restore |
| POST | `/api/guilds/:guild_id/pages/reorder` | Reorder |
| PUT | `/api/guilds/:guild_id/pages/:id/visibility` | Set role visibility |

### Page Acceptance

| Method | Path | Description |
|--------|------|-------------|
| POST | `/api/pages/:id/accept` | Record user acceptance |
| GET | `/api/pages/pending-acceptance` | Pages user needs to accept |

### Platform Admin

| Method | Path | Description |
|--------|------|-------------|
| GET | `/api/admin/users` | List users (for promoting) |
| POST | `/api/admin/users/:id/promote` | Grant admin role |
| DELETE | `/api/admin/users/:id/demote` | Revoke admin role |

### Security Measures

- Rate limiting: 10 creates/hour, 30 updates/hour, 100 reads/minute
- Slug validation: `^[a-z0-9][a-z0-9\-]*[a-z0-9]$`
- Reserved slugs: admin, api, new, edit, delete, settings, create, update, list, all, me, system
- Content limit: 100KB
- 7-day cooldown on deleted slugs
- Password confirmation for admin promotions
- Last-admin protection

---

## Frontend Components

### File Structure

```
src/components/pages/
├── AcceptanceManager.tsx    # Manages acceptance flow
├── ImageUploader.tsx        # Drag-drop image upload
├── MarkdownCheatSheet.tsx   # Quick reference panel
├── MarkdownPreview.tsx      # Secure markdown + mermaid renderer
├── PageAcceptanceModal.tsx  # Accept ToS modal
├── PageEditor.tsx           # Side-by-side editor
├── PageItem.tsx             # Sortable page list item
├── PageSection.tsx          # Sidebar pages section
├── PageSettings.tsx         # Visibility & acceptance settings
├── PageView.tsx             # Full page display
└── PlatformPagesCard.tsx    # Home view platform info
```

### Pages Store

Location: `src/stores/pages.ts`

State:
- `platformPages: PageListItem[]`
- `guildPages: Record<string, PageListItem[]>`
- `currentPage: Page | null`
- `pendingAcceptance: PageListItem[]`
- `isLoading, isPlatformLoading, error`

Actions:
- `loadPlatformPages()`
- `loadGuildPages(guildId)`
- `loadPage(guildId, slug)`
- `createPage(guildId, data)`
- `updatePage(pageId, data)`
- `deletePage(pageId)`
- `reorderPages(guildId, pageIds)`
- `acceptPage(pageId)`
- `loadPendingAcceptance()`

### Markdown Rendering Security

- HTML sanitization with allowlist (p, br, strong, em, code, pre, h1-h6, ul, ol, li, blockquote, hr, a, img, table, etc.)
- URL protocol allowlist (http, https, mailto, relative only)
- Mermaid with `securityLevel: "strict"`
- Image upload: 5MB limit, JPEG/PNG/GIF/WebP only

### Editor Features

- Side-by-side layout (editor + live preview)
- Toolbar: Bold, Italic, Strikethrough, Image, Mermaid diagram
- Auto-generated slug from title (editable)
- Markdown cheat sheet panel
- Settings panel: acceptance toggle, role visibility
- Unsaved changes warning
- Content length limit: 100KB

### UI Integration

**Sidebar (Guild View):**
- Collapsible "Pages" section above channel list
- Hidden for users when no pages exist
- Always visible for users with ManagePages permission
- Drag-and-drop reordering for admins

**Home View:**
- "Platform Info" card with list of platform pages
- "Action Required" badge for unaccepted pages
- "+ Add" button for platform admins

**Acceptance Flow:**
- Modal overlay for pages requiring acceptance
- Platform pages are blocking (must accept or logout)
- Guild pages are non-blocking (can defer)
- Must scroll to bottom before accepting

---

## Routing

```typescript
// Guild page routes
'/guild/:guildId/pages/:slug'       // View page
'/guild/:guildId/pages/:slug/edit'  // Edit page
'/guild/:guildId/pages/new'         // Create page

// Platform page routes
'/pages/:slug'                      // View page
'/pages/:slug/edit'                 // Edit page
'/pages/new'                        // Create page
```

---

## New Permission

Add to guild role system:

```rust
pub enum GuildPermission {
    // ... existing
    ManagePages,  // Create, edit, delete, reorder pages
}
```

---

## Implementation Order

1. **Database** - Create migration with all tables
2. **Backend API** - Platform admin + pages CRUD + acceptance
3. **Tauri Commands** - Add command wrappers
4. **Store** - Create pages store
5. **Markdown Renderer** - Secure renderer with mermaid
6. **Page Editor** - Side-by-side with all features
7. **Sidebar Integration** - PageSection component
8. **Home View Integration** - PlatformPagesCard
9. **Acceptance Flow** - Modal + manager
10. **Testing** - Unit + integration tests

---

## Future Enhancements

- Full version history with diff view
- Archive feature (unpublish without delete)
- Public pages (accessible without login)
- Page templates
- Search within pages
- Table of contents generation
- PDF export
