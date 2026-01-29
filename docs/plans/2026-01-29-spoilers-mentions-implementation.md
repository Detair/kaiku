# Content Spoilers & Enhanced Mentions — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add `||spoiler||` syntax for hiding sensitive content behind a blur, and add `MENTION_EVERYONE` permission (bit 23) so guild admins can control who uses `@everyone`/`@here`. Also add visual highlighting of mentions in rendered messages.

**Architecture:** Spoilers are parsed client-side in the markdown pipeline (pre-DOMPurify). Mention permission enforcement happens server-side in the message create handler. Mention highlighting is done client-side with regex replacement during rendering.

**Tech Stack:** Rust (server permission enforcement), Solid.js (Spoiler component, mention highlighting), marked.js (custom extension), existing CSS blur utilities.

---

## Context

### Existing Infrastructure (DO NOT recreate)

| Component | Location | What it does |
|-----------|----------|--------------|
| `detect_mention_type()` | `server/src/chat/messages.rs:165-196` | Detects @everyone/@here/@username in content |
| `MentionType` enum | `server/src/chat/messages.rs:105-114` | Direct, Everyone, Here variants |
| `GuildPermissions` bitflags | `server/src/permissions/guild.rs` | Bits 0-22 allocated |
| `PermissionBits` (client) | `client/src/lib/permissionConstants.ts` | Client mirror of server permissions |
| `marked.js` + DOMPurify | `client/src/components/messages/MessageItem.tsx` | Markdown rendering pipeline |
| `PURIFY_CONFIG` | `MessageItem.tsx:31-36` | Allowed HTML tags/attributes |
| `contentBlocks()` memo | `MessageItem.tsx:91-136` | Splits content into code blocks + text |
| Notification system | `client/src/stores/websocket.ts:73-109` | Plays sounds on mentions |

### What's Missing

1. **Spoiler syntax** — No `||spoiler||` parsing or blur rendering
2. **MENTION_EVERYONE permission** — Any user can @everyone/@here (bit 23 is free)
3. **Server-side mention enforcement** — `detect_mention_type()` detects but doesn't validate permissions
4. **Visual mention highlighting** — @mentions render as plain text, not highlighted
5. **Stripped mentions** — When user lacks permission, @everyone should be silently stripped

---

## Files to Modify

### Server
| File | Changes |
|------|---------|
| `server/src/permissions/guild.rs` | Add `MENTION_EVERYONE` bit 23 |
| `server/src/chat/messages.rs` | Enforce mention permission in `create()` handler |

### Client
| File | Changes |
|------|---------|
| `client/src/lib/permissionConstants.ts` | Add `MENTION_EVERYONE` bit 23 |
| `client/src/components/messages/MessageItem.tsx` | Add spoiler + mention parsing to content pipeline |
| `client/src/components/messages/SpoilerText.tsx` | **NEW** — Click-to-reveal spoiler component |
| `client/src/styles/global.css` | Mention highlight CSS classes |

---

## Implementation Tasks

### Task 1: MENTION_EVERYONE Permission Bit (Server)

**Files:**
- Modify: `server/src/permissions/guild.rs`

**Step 1: Add the permission bit**

Add after `SCREEN_SHARE` in the bitflags macro:

```rust
// === Mentions (bit 23) ===
/// Allows using @everyone and @here mentions
const MENTION_EVERYONE   = 1 << 23;
```

**Step 2: Add to default presets**

- `MODERATOR_DEFAULT`: Add `.union(Self::MENTION_EVERYONE)` — moderators can @everyone
- `OFFICER_DEFAULT`: Already inherits from MODERATOR_DEFAULT
- `EVERYONE_DEFAULT`: Do NOT add — regular users cannot @everyone by default

**Step 3: Add to EVERYONE_FORBIDDEN**

In the `EVERYONE_FORBIDDEN` constant, add:

```rust
.union(Self::MENTION_EVERYONE)
```

This prevents the @everyone role from having this permission (must be granted via a specific role).

**Step 4: Add test**

Add a test assertion:

```rust
assert_eq!(GuildPermissions::MENTION_EVERYONE.bits(), 1 << 23);
```

And verify it's forbidden for @everyone:

```rust
assert!(!everyone.has(GuildPermissions::MENTION_EVERYONE));
```

**Verification:**
```bash
cd server && cargo test -- permissions
```

---

### Task 2: Server-Side Mention Enforcement

**Files:**
- Modify: `server/src/chat/messages.rs`

**Purpose:** When a user sends a message containing `@everyone` or `@here`, check if they have the `MENTION_EVERYONE` permission. If not, strip the mentions from the content before saving.

**Step 1: Import permission checking**

Add the required imports:

```rust
use crate::permissions::guild::GuildPermissions;
use crate::permissions::check_guild_permission;
```

**Step 2: Add mention stripping in `create()` handler**

After the content validation and before the database insert, add:

```rust
// Strip @everyone/@here if user lacks MENTION_EVERYONE permission
let final_content = if !message_body.encrypted {
    let has_mass_mention = message_body.content.contains("@everyone")
        || message_body.content.contains("@here");

    if has_mass_mention {
        // Check if this is a guild channel
        let channel = sqlx::query_as!(
            Channel,
            "SELECT * FROM channels WHERE id = $1",
            channel_id
        )
        .fetch_one(&state.db)
        .await?;

        if let Some(guild_id) = channel.guild_id {
            let can_mention = check_guild_permission(
                &state.db,
                guild_id,
                auth.id,
                GuildPermissions::MENTION_EVERYONE,
            )
            .await
            .unwrap_or(false);

            if !can_mention {
                // Replace @everyone and @here with escaped versions
                // Zero-width space breaks mention detection without visual change
                message_body.content
                    .replace("@everyone", "@\u{200B}everyone")
                    .replace("@here", "@\u{200B}here")
            } else {
                message_body.content.clone()
            }
        } else {
            // DMs: @everyone/@here are always allowed (no permission concept)
            message_body.content.clone()
        }
    } else {
        message_body.content.clone()
    }
} else {
    // Encrypted: can't inspect content
    message_body.content.clone()
};
```

**Key design:** Uses zero-width space insertion to visually preserve the text while preventing the server-side `detect_mention_type()` from recognizing it as a mention. This way:
- The message still displays `@everyone` visually
- But `detect_mention_type()` won't match it (breaks the regex)
- No notification sound triggers for unpermitted mentions

**Step 3: Use `final_content` for database insert and mention detection**

Pass `final_content` instead of `message_body.content` to both `db::create_message()` and `detect_mention_type()`.

**Note:** The `check_guild_permission` function should already exist in the permissions module. If it doesn't exist as a standalone function, the implementer should check how permission checks are done in other handlers (e.g., `kick_member`) and follow the same pattern. The typical pattern involves:
1. Get user's guild member record
2. Get user's roles
3. Compute effective permissions (role union + overrides)
4. Check if the target bit is set

**Verification:**
```bash
cd server && cargo test
```

---

### Task 3: MENTION_EVERYONE Permission Bit (Client)

**Files:**
- Modify: `client/src/lib/permissionConstants.ts`

**Step 1: Add the bit constant**

Add after `MANAGE_EMOJIS_AND_STICKERS`:

```typescript
// Mentions (bit 23)
MENTION_EVERYONE: 1 << 23,
```

**Step 2: Add to category type**

Add `"mentions"` to the `PermissionCategory` union type:

```typescript
export type PermissionCategory =
  | "content"
  | "voice"
  | "moderation"
  | "guild_management"
  | "invites"
  | "pages"
  | "mentions";
```

**Step 3: Add permission definition**

Add to the `PERMISSIONS` array:

```typescript
{
  key: "MENTION_EVERYONE",
  bit: PermissionBits.MENTION_EVERYONE,
  name: "Mention Everyone",
  description: "Allows using @everyone and @here to notify all members",
  category: "mentions",
  forbiddenForEveryone: true,
},
```

**Step 4: Add category display name**

Add to `CATEGORY_NAMES`:

```typescript
mentions: "Mentions",
```

**Step 5: Add to EVERYONE_FORBIDDEN**

Add `PermissionBits.MENTION_EVERYONE` to the `EVERYONE_FORBIDDEN` constant.

**Step 6: Add to preset defaults**

Add to `MODERATOR_DEFAULT`:

```typescript
PermissionBits.MENTION_EVERYONE;
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 4: SpoilerText Component

**Files:**
- Create: `client/src/components/messages/SpoilerText.tsx`

**Purpose:** A click-to-reveal inline component that blurs content until clicked.

```tsx
/**
 * SpoilerText -- Click-to-reveal spoiler content.
 * Renders inline, blurred by default, revealed on click.
 */
import { Component, createSignal } from "solid-js";

interface SpoilerTextProps {
  /** The hidden content (HTML string from markdown parsing) */
  html: string;
}

const SpoilerText: Component<SpoilerTextProps> = (props) => {
  const [revealed, setRevealed] = createSignal(false);

  return (
    <span
      class={`
        inline rounded px-0.5 cursor-pointer transition-all duration-200
        ${revealed()
          ? "bg-white/10"
          : "bg-surface-layer2 select-none"
        }
      `}
      style={{
        filter: revealed() ? "none" : "blur(4px)",
      }}
      onClick={() => setRevealed(!revealed())}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          setRevealed(!revealed());
        }
      }}
      role="button"
      tabIndex={0}
      title={revealed() ? "Click to hide" : "Click to reveal spoiler"}
    >
      <span innerHTML={props.html} />
    </span>
  );
};

export default SpoilerText;
```

**Design decisions:**
- Uses CSS `filter: blur(4px)` for content blurring
- `select-none` prevents selecting blurred text
- Toggles on click (can re-hide)
- Accessible: keyboard support + role="button"
- Accepts HTML string so markdown formatting works inside spoilers

**Verification:**
```bash
cd client && bun run check
```

---

### Task 5: Spoiler + Mention Parsing in Message Rendering

**Files:**
- Modify: `client/src/components/messages/MessageItem.tsx`
- Modify: `client/src/styles/global.css`

**Purpose:** Extend the content rendering pipeline to:
1. Parse `||spoiler text||` syntax into interactive SpoilerText components
2. Highlight `@everyone`, `@here`, and `@username` mentions with visual styling

This is the most complex task. The existing pipeline is:

```
content -> split by code blocks -> marked.parse() -> DOMPurify.sanitize() -> innerHTML
```

The new pipeline becomes:

```
content -> split by code blocks -> for each text segment:
  1. Split by ||spoiler|| markers
  2. For non-spoiler parts: highlightMentions() -> marked.parse() -> sanitize()
  3. For spoiler parts: highlightMentions() -> marked.parse() -> sanitize() -> SpoilerText
```

**Step 1: Import SpoilerText**

```typescript
import SpoilerText from "./SpoilerText";
```

**Step 2: Update PURIFY_CONFIG**

Add `mark` and `span` to `ALLOWED_TAGS`:

```typescript
ALLOWED_TAGS: ['p', 'br', 'strong', 'em', 'code', 'pre', 'a', 'ul', 'ol', 'li',
  'blockquote', 'h1', 'h2', 'h3', 'h4', 'h5', 'h6', 'hr', 'del', 's',
  'table', 'thead', 'tbody', 'tr', 'th', 'td', 'mark', 'span'],
```

Add `class` to `ALLOWED_ATTR`:

```typescript
ALLOWED_ATTR: ['href', 'target', 'rel', 'class'],
```

**Step 3: Add mention highlighting function**

Before the component, add:

```typescript
/**
 * Highlight @mentions in text before markdown parsing.
 * Wraps @everyone, @here, and @username in styled <mark> tags.
 */
function highlightMentions(text: string): string {
  // @everyone and @here -- high-visibility
  let result = text.replace(
    /@(everyone|here)\b/g,
    '<mark class="mention-everyone">@$1</mark>'
  );
  // @username -- normal mention (2-32 chars, not inside an HTML tag)
  result = result.replace(
    /@(\w{2,32})(?![^<]*>)/g,
    (match, username) => {
      if (username === "everyone" || username === "here") return match;
      return `<mark class="mention-user">@${username}</mark>`;
    }
  );
  return result;
}
```

**Step 4: Add CSS classes for mentions**

Add to `client/src/styles/global.css`:

```css
/* Mention highlighting */
.mention-everyone {
  background-color: rgba(99, 102, 241, 0.2);
  color: var(--color-accent-primary);
  padding: 0 2px;
  border-radius: 3px;
  font-weight: 600;
}

.mention-user {
  background-color: rgba(99, 102, 241, 0.15);
  color: var(--color-accent-primary);
  padding: 0 2px;
  border-radius: 3px;
  cursor: pointer;
}

.mention-user:hover {
  background-color: rgba(99, 102, 241, 0.3);
}
```

**Step 5: Modify ContentBlock type**

Add a spoiler block variant:

```typescript
interface SpoilerBlock {
  type: 'spoiler';
  html: string;
}

type ContentBlock = CodeBlockData | TextBlock | SpoilerBlock;
```

**Step 6: Add processTextSegment function**

This replaces direct `marked.parse()` calls for text segments:

```typescript
/**
 * Process a text segment: split by spoilers, highlight mentions, parse markdown.
 */
function processTextSegment(text: string): ContentBlock[] {
  const blocks: ContentBlock[] = [];
  const spoilerRegex = /\|\|(.+?)\|\|/gs;
  let lastIdx = 0;
  let match;

  while ((match = spoilerRegex.exec(text)) !== null) {
    // Text before spoiler
    if (match.index > lastIdx) {
      const before = text.substring(lastIdx, match.index);
      if (before.trim()) {
        const processed = highlightMentions(before);
        const html = sanitizeHtml(marked.parse(processed, { async: false }) as string);
        blocks.push({ type: 'text', html });
      }
    }

    // Spoiler content (also parse markdown inside)
    const spoilerContent = match[1];
    const processed = highlightMentions(spoilerContent);
    const html = sanitizeHtml(marked.parse(processed, { async: false }) as string);
    blocks.push({ type: 'spoiler', html });

    lastIdx = match.index + match[0].length;
  }

  // Remaining text after last spoiler
  if (lastIdx < text.length) {
    const remaining = text.substring(lastIdx);
    if (remaining.trim()) {
      const processed = highlightMentions(remaining);
      const html = sanitizeHtml(marked.parse(processed, { async: false }) as string);
      blocks.push({ type: 'text', html });
    }
  }

  // No spoilers found -- process normally
  if (blocks.length === 0) {
    const processed = highlightMentions(text);
    const html = sanitizeHtml(marked.parse(processed, { async: false }) as string);
    blocks.push({ type: 'text', html });
  }

  return blocks;
}
```

**Step 7: Update contentBlocks() memo**

Replace all direct `marked.parse()` / `sanitizeHtml()` calls for text segments with `processTextSegment()`. The code block extraction logic stays the same. Where previously:

```typescript
const html = sanitizeHtml(marked.parse(text, { async: false }) as string);
blocks.push({ type: 'text', html });
```

Now use:

```typescript
blocks.push(...processTextSegment(text));
```

Do this for all three places where text blocks are created (before code block, after code block, and the fallback when no code blocks found).

**Step 8: Update the render template**

Replace the existing `<For>` block:

```tsx
<For each={contentBlocks()}>
  {(block) => (
    <>
      <Show when={block.type === 'code'}>
        <CodeBlock language={(block as CodeBlockData).language}>
          {(block as CodeBlockData).code}
        </CodeBlock>
      </Show>
      <Show when={block.type === 'text'}>
        <div innerHTML={(block as TextBlock).html} />
      </Show>
      <Show when={block.type === 'spoiler'}>
        <SpoilerText html={(block as SpoilerBlock).html} />
      </Show>
    </>
  )}
</For>
```

**Verification:**
```bash
cd client && bun run check
```

---

### Task 6: CHANGELOG Update

**Files:**
- Modify: `CHANGELOG.md`

Add under `### Added` in the `[Unreleased]` section:

```markdown
- Content Spoilers with `||spoiler||` syntax
  - Blurred text that reveals on click
  - Supports markdown formatting inside spoilers
  - Accessible with keyboard (Enter/Space to toggle)
- Enhanced Mentions
  - `MENTION_EVERYONE` permission (bit 23) controls who can use @everyone and @here
  - Unpermitted @everyone/@here silently stripped server-side
  - Visual highlighting for @everyone, @here, and @username mentions
  - Moderator+ roles have MENTION_EVERYONE by default
```

Add under `### Security`:

```markdown
- Prevented `@everyone` role from being assigned `MENTION_EVERYONE` permission via API validation
```

**Verification:**
```bash
cd server && cargo check && cargo test
cd client && bun run check
```

---

## Verification

### Server
```bash
cd server && cargo check && cargo test
```

### Client
```bash
cd client && bun run check
```

### Manual Testing

**Spoilers:**
1. Send message: `This is a ||secret message|| in chat`
2. Verify "secret message" appears blurred
3. Click the blurred text -- reveals content
4. Click again -- re-hides content
5. Test keyboard: Tab to spoiler, press Enter -- reveals
6. Test spoiler with markdown: `||**bold** spoiler||` -- verify bold renders inside
7. Test multiple spoilers: `||first|| and ||second||` -- both work independently

**Mention Permission:**
1. As guild owner (has all permissions): send `@everyone hello` -- renders highlighted, triggers notification
2. As regular member (no MENTION_EVERYONE): send `@everyone hello` -- renders as plain text, no notification
3. As moderator (has MENTION_EVERYONE by default): send `@here` -- works, highlighted
4. In DM: `@everyone` -- always works (no guild permission concept)
5. In guild settings > Roles: verify MENTION_EVERYONE toggle appears in "Mentions" category

**Mention Highlighting:**
1. Send `@everyone test` -- "@everyone" has accent-primary background highlight
2. Send `@here test` -- "@here" similarly highlighted
3. Send `@username test` -- "@username" highlighted with lighter style
4. Send `@nonexistent` -- still highlighted (client can't validate usernames inline)
