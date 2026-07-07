# Forum & Announcement Channels — Design

> Status: design, awaiting approval. 2026-07-07, gap #3. Scope decision: forum
> **and** announcement channels together, including cross-guild announcement
> follow/crosspost.

## Context

Channels are `Text` or `Voice` only (`db::ChannelType`, string-mapped in
`chat/channels.rs`). Communities miss two Discord channel types most: **forum
channels** (organized topic posts) and **announcement channels** (broadcast +
follow). Forum posts map cleanly onto Kaiku's existing thread system — `messages`
already has `parent_id` (self-FK, cascade) and `thread_reply_count`
(`20260206000000_message_threads.sql`), so a forum post *is* a thread starter.
Announcement channels are a write-restricted text channel plus a follow/crosspost
mechanism, the only genuinely new machinery here.

## Goals

- **Forum channel** (`ChannelType::Forum`): contains **posts**, each a
  title + tags + a root message that opens a thread. Members browse a card list,
  filter by tag, and reply inside a post (reusing thread replies).
- **Announcement channel** (`ChannelType::Announcement`): a text channel where
  posting needs a permission bit; any guild can **follow** it, so new
  announcements **crosspost** into a chosen channel in the follower's guild.

## Non-Goals (YAGNI)

- Forum post reactions/upvote sorting beyond recent/active — start with
  recent-activity + tag filter.
- Default forum layout toggles (list vs gallery) — list only for v1.
- Announcement scheduling — that's the scheduled-events/scheduled-messages track.

## Data Model

`channel_type` is a **Postgres ENUM** (`CREATE TYPE channel_type AS ENUM
('text','voice','dm')`), not a VARCHAR — so new variants are added with
`ALTER TYPE … ADD VALUE`. Caveat: `ALTER TYPE … ADD VALUE` **cannot run in the
same transaction that uses the new value**, and sqlx runs each migration in a
transaction. So the enum extension is its **own** migration, committed before any
migration/code references `'forum'`/`'announcement'`:

```sql
-- migration: 20260707000002a_channel_type_forum_announcement.sql  (enum only)
ALTER TYPE channel_type ADD VALUE IF NOT EXISTS 'forum';
ALTER TYPE channel_type ADD VALUE IF NOT EXISTS 'announcement';
```

```sql
-- migration: 20260707000002b_forum_announcement_channels.sql  (tables)

-- Forum posts. The post's root message lives in `messages` (parent_id NULL,
-- channel_id = the forum channel); thread replies hang off it as today.
CREATE TABLE forum_posts (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id  UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    root_message_id UUID NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
    title       VARCHAR(128) NOT NULL,
    author_id   UUID REFERENCES users(id) ON DELETE SET NULL,
    pinned      BOOLEAN NOT NULL DEFAULT FALSE,
    locked      BOOLEAN NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_activity_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);
CREATE INDEX idx_forum_posts_channel ON forum_posts(channel_id, last_activity_at DESC);

-- Forum tags: a small per-channel tag set; posts reference them.
CREATE TABLE forum_tags (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    name VARCHAR(32) NOT NULL,
    emoji VARCHAR(64),
    UNIQUE (channel_id, name)
);
CREATE TABLE forum_post_tags (
    post_id UUID NOT NULL REFERENCES forum_posts(id) ON DELETE CASCADE,
    tag_id  UUID NOT NULL REFERENCES forum_tags(id)  ON DELETE CASCADE,
    PRIMARY KEY (post_id, tag_id)
);

-- Announcement follows: a follower guild subscribes one of its channels to a
-- source announcement channel.
CREATE TABLE announcement_follows (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    source_channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    target_channel_id UUID NOT NULL REFERENCES channels(id) ON DELETE CASCADE,
    created_by UUID REFERENCES users(id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    UNIQUE (source_channel_id, target_channel_id)
);
```

`last_activity_at` bumps on new replies so the card list can sort by recent
activity without scanning messages.

**Rust-side code changes:** the sqlx-derived `db::ChannelType` enum gains
`Forum` and `Announcement` variants, and the string mapping in
`chat/channels.rs` (`"text" => Text`, `"voice" => Voice`, and the reverse) gains
the two new cases. The existing `Voice`-specific branch in channel creation
stays; `Forum` gets its own post-creation branch.

## Permissions

Verified against `permissions/guild.rs`: `GuildPermissions` is a `u64` bitflags
with bits 0–25 in use, so the new bits below take **1<<26** (`MANAGE_POSTS`) and
**1<<27** (`SEND_ANNOUNCEMENTS`) — no collisions.

- New permission bit **`MANAGE_POSTS`** (forum moderation: pin/lock/delete
  others' posts, manage tags). Creating a post needs `SEND_MESSAGES` in the
  forum channel; replying needs the same, honoring `locked`.
- Announcement posting reuses a new **`SEND_ANNOUNCEMENTS`** bit distinct from
  `SEND_MESSAGES`. Following a source channel requires `MANAGE_CHANNELS` in the
  **follower** guild and `VIEW_CHANNEL` on the source (public discovery only for
  now — a source must be in a discoverable guild or reachable by invite).

## Behavior

**Forum posts** — creating a post is one transaction: insert the root `messages`
row (parent_id NULL, the forum channel), the `forum_posts` row, and tag links.
Replies use the **existing thread-reply endpoint** unchanged; a reply bumps
`forum_posts.last_activity_at` and the existing `thread_reply_count`. Deleting
the root message cascades the post; deleting the channel cascades everything.

**Announcement crosspost** — publishing a message in an announcement channel (a
normal message create, gated by `SEND_ANNOUNCEMENTS`) triggers a fan-out: for
each `announcement_follows` row with that source, create a copy message in the
target channel attributed to the source (a "forwarded from #source" header,
original author preserved in metadata). Fan-out runs on the existing
Redis-backed async path (like webhook delivery) so a slow follower can't block
the publish. Follower unsubscribes via `DELETE` on the follow row.

## Real-Time (WS)

- Forum: reuse `MessageNew`/`MessageEdit` for post root + replies; add
  `ForumPostCreated { channel_id, post }` and `ForumPostUpdated` so the card list
  updates without refetch.
- Announcement crossposts arrive in target channels as ordinary `MessageNew`
  events (they *are* messages), so no client change is needed there.

## API Surface

New module `server/src/chat/forum.rs` (Tier-1 layout):
`GET /api/channels/{id}/posts` (paginated, `?tag=` filter, sort recent/active),
`POST /api/channels/{id}/posts`, `PATCH/DELETE /api/forum/posts/{post_id}`
(pin/lock/delete), tag CRUD under the channel.
Announcement follow: `POST /api/channels/{target}/follow {source_channel_id}`,
`DELETE …/follow/{id}`, `GET …/followers` (source-side).

## Client

- **`components/channels/`** gains a forum channel view: post card list (title,
  tags, author, reply count, last activity), tag filter chips, a "New Post"
  composer (title + tags + body), and a post view that reuses the existing
  thread UI for replies. Channel-create modal gains "Forum" and "Announcement"
  types (extends the existing type picker).
- Announcement channels render like text with a "Following" affordance and a
  publish button gated on `SEND_ANNOUNCEMENTS`.

## Edge Cases

- Following a channel that later becomes private/deleted → follow row
  cascade-removed or crosspost skipped with a logged warning.
- Crosspost loops (A follows B follows A) → guard: a crossposted message is
  flagged and never re-crossposted.
- Forum post with all tags deleted → post keeps working, untagged.
- Locked post → replies rejected with `403`; author/`MANAGE_POSTS` can still act.
- Converting an existing text channel to forum → out of scope; forum channels
  are created as forum.

## Testing

- Forum: create post (root message + post + tags in one tx), reply bumps
  activity + count, tag filter, pin/lock permission gates, cascade on
  channel/root delete.
- Announcement: `SEND_ANNOUNCEMENTS` gate, follow creates crossposts to targets,
  loop guard, unfollow stops crossposts, slow follower doesn't block publish.

## Rollout

Additive channel types + tables; existing text/voice channels untouched. The
`channels_type_check` change is a constraint swap (safe, no data change). Ship
server + client together so the new channel types render. Announcement
cross-guild follow is the riskiest slice — it can be feature-flagged off while
forum ships, if we want to de-risk the crosspost fan-out separately.
