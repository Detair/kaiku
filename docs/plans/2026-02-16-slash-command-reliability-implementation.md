# Slash Command Reliability & /ping — Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Fix end-to-end slash command reliability — registration uniqueness, listing consistency, response delivery to users, gateway error contract, frontend fixes, and a built-in /ping reference command.

**Architecture:** The critical fix is response delivery: after a bot stores its response in Redis, a spawned tokio task relays it to the invoking user via their existing WebSocket connection. Non-ephemeral responses are persisted as bot-authored messages; ephemeral ones are transient user-only events. Registration gets a partial unique index and batch duplicate detection. Listing drops `DISTINCT ON` to surface all providers. Frontend shows separate autocomplete entries for ambiguous commands.

**Tech Stack:** Rust (axum, sqlx, tokio, fred), Solid.js, PostgreSQL, Valkey/Redis.

---

## Task 1: Add global command uniqueness index

**Files:**
- Create: `server/migrations/20260216000000_slash_command_uniqueness.sql`

**Step 1: Write the migration**

```sql
-- Enforce uniqueness for global commands (guild_id IS NULL) per application.
-- Guild-scoped commands already have UNIQUE(application_id, guild_id, name).
CREATE UNIQUE INDEX IF NOT EXISTS idx_slash_commands_global_app_name
    ON slash_commands (application_id, name)
    WHERE guild_id IS NULL;
```

**Step 2: Verify migration compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo sqlx prepare --check 2>&1 || echo "Prepare needed"`

If prepare fails, that's expected — we just need the migration file to exist. The actual DB apply happens in tests.

**Step 3: Commit**

```
feat(db): add global command uniqueness index

Partial unique index on (application_id, name) WHERE guild_id IS NULL
prevents duplicate global commands per bot application.
```

---

## Task 2: Add batch duplicate detection and 409 conflict handling in registration

**Files:**
- Modify: `server/src/api/commands.rs:17-56` (add `DuplicateName` error variant)
- Modify: `server/src/api/commands.rs:161-251` (add duplicate check in `register_commands`)

**Step 1: Write the failing test**

Add to `server/tests/bot_ecosystem_test.rs`:

```rust
#[sqlx::test]
async fn test_register_commands_rejects_batch_duplicates(pool: PgPool) {
    let state = setup_test_state(pool).await;
    let (jwt, app_id) = create_test_app(&state).await;

    let res = state.client
        .put(&format!("/api/applications/{app_id}/commands"))
        .bearer_auth(&jwt)
        .json(&serde_json::json!({
            "commands": [
                {"name": "ping", "description": "First ping", "options": []},
                {"name": "ping", "description": "Dupe ping", "options": []}
            ]
        }))
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 409);
}
```

**Step 2: Run test to verify it fails**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test --test bot_ecosystem_test test_register_commands_rejects_batch_duplicates -- --nocapture`

Expected: FAIL (currently inserts both, no duplicate check)

**Step 3: Add `DuplicateName` error variant**

In `server/src/api/commands.rs`, add to `CommandError` enum (after `InvalidDescription`):

```rust
    /// Duplicate command name in batch.
    #[error("Duplicate command name in batch: {0}")]
    DuplicateName(String),
```

Add to the `From<CommandError> for (StatusCode, String)` match:

```rust
            CommandError::DuplicateName(_) => (StatusCode::CONFLICT, err.to_string()),
```

**Step 4: Add batch duplicate detection in `register_commands`**

In `server/src/api/commands.rs`, after the validation loop (line ~191), add:

```rust
    // Check for duplicate names within the batch
    let mut seen_names = std::collections::HashSet::new();
    for cmd in &req.commands {
        if !seen_names.insert(&cmd.name) {
            return Err(CommandError::DuplicateName(cmd.name.clone()).into());
        }
    }
```

**Step 5: Run test to verify it passes**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test --test bot_ecosystem_test test_register_commands_rejects_batch_duplicates -- --nocapture`

Expected: PASS

**Step 6: Commit**

```
feat(api): reject duplicate command names in registration batch

Adds HashSet-based duplicate detection before DB insert and maps
uniqueness violations to typed 409 Conflict responses.
```

---

## Task 3: Fix guild command listing — remove DISTINCT ON, add application_id

**Files:**
- Modify: `server/src/guild/types.rs:190-194` (extend `GuildCommandInfo`)
- Modify: `server/src/guild/handlers.rs:787-823` (rewrite query)
- Modify: `client/src/lib/api/bots.ts` (extend `GuildCommand` type)

**Step 1: Write the failing test**

Add to `server/tests/bot_ecosystem_test.rs`:

```rust
#[sqlx::test]
async fn test_list_guild_commands_shows_all_providers(pool: PgPool) {
    let state = setup_test_state(pool).await;
    let (jwt, app_id_1) = create_test_app(&state, "BotA").await;
    let (_, app_id_2) = create_test_app(&state, "BotB").await;
    let guild_id = create_test_guild(&state, &jwt).await;

    // Both bots register /ping
    register_command(&state, &jwt, app_id_1, "ping", "Ping from A").await;
    register_command(&state, &jwt, app_id_2, "ping", "Ping from B").await;
    install_bot(&state, &jwt, guild_id, app_id_1).await;
    install_bot(&state, &jwt, guild_id, app_id_2).await;

    let res = state.client
        .get(&format!("/api/guilds/{guild_id}/commands"))
        .bearer_auth(&jwt)
        .send()
        .await
        .unwrap();

    assert_eq!(res.status(), 200);
    let commands: Vec<serde_json::Value> = res.json().await.unwrap();
    // Should show BOTH entries, not deduplicate
    let ping_commands: Vec<_> = commands.iter()
        .filter(|c| c["name"] == "ping")
        .collect();
    assert_eq!(ping_commands.len(), 2);
}
```

Note: The exact helper function signatures above are pseudocode. Adapt to match the existing test helper patterns in `bot_ecosystem_test.rs`.

**Step 2: Run test to verify it fails**

Expected: FAIL (DISTINCT ON returns only 1 result)

**Step 3: Extend `GuildCommandInfo`**

In `server/src/guild/types.rs`, update the struct:

```rust
pub struct GuildCommandInfo {
    pub name: String,
    pub description: String,
    pub bot_name: String,
    pub application_id: Uuid,
    pub is_ambiguous: bool,
}
```

**Step 4: Rewrite the listing query**

Replace `list_guild_commands` in `server/src/guild/handlers.rs` (lines 787-823):

```rust
pub async fn list_guild_commands(
    State(state): State<AppState>,
    auth: AuthUser,
    Path(guild_id): Path<Uuid>,
) -> Result<Json<Vec<GuildCommandInfo>>, GuildError> {
    let is_member = db::is_guild_member(&state.db, guild_id, auth.id).await?;
    if !is_member {
        return Err(GuildError::Forbidden);
    }

    // Return all commands from installed bots (no DISTINCT ON).
    // Guild-scoped commands sort before global; within same scope, sort by created_at.
    let rows: Vec<(String, String, String, Uuid)> = sqlx::query_as(
        r"SELECT sc.name, sc.description, ba.name as bot_name, ba.id as application_id
           FROM slash_commands sc
           INNER JOIN bot_applications ba ON sc.application_id = ba.id
           INNER JOIN guild_bot_installations gbi ON ba.id = gbi.application_id
           WHERE gbi.guild_id = $1 AND (sc.guild_id = $1 OR sc.guild_id IS NULL)
           ORDER BY sc.name, (sc.guild_id IS NULL), sc.created_at",
    )
    .bind(guild_id)
    .fetch_all(&state.db)
    .await?;

    // Compute ambiguity: count how many distinct apps provide each command name.
    let mut name_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for (name, _, _, _) in &rows {
        *name_counts.entry(name.as_str()).or_insert(0) += 1;
    }

    let result: Vec<GuildCommandInfo> = rows
        .into_iter()
        .map(|(name, description, bot_name, application_id)| {
            let is_ambiguous = name_counts.get(name.as_str()).copied().unwrap_or(0) > 1;
            GuildCommandInfo { name, description, bot_name, application_id, is_ambiguous }
        })
        .collect();

    Ok(Json(result))
}
```

**Step 5: Update frontend type**

In `client/src/lib/api/bots.ts`, update:

```typescript
export interface GuildCommand {
    name: string;
    description: string;
    bot_name: string;
    application_id: string;
    is_ambiguous: boolean;
}
```

**Step 6: Run tests**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test --test bot_ecosystem_test test_list_guild_commands -- --nocapture`

Expected: PASS

**Step 7: Commit**

```
feat(api): show all command providers in guild listing

Removes DISTINCT ON from list_guild_commands, returns all commands
from all installed bots with application_id and is_ambiguous flag.
Fixes inconsistency between listing and invocation behavior.
```

---

## Task 4: Add ServerEvent variants for command responses

**Files:**
- Modify: `server/src/ws/mod.rs:224-710` (add variants to `ServerEvent` enum)

**Step 1: Add new event variants**

Add before the closing `}` of the `ServerEvent` enum (before line 710):

```rust
    /// Bot command response delivered to invoking user
    CommandResponse {
        /// Interaction ID.
        interaction_id: Uuid,
        /// Response content from the bot.
        content: String,
        /// Command name that was invoked.
        command_name: String,
        /// Bot display name.
        bot_name: String,
        /// Channel where command was invoked.
        channel_id: Uuid,
        /// Whether response is ephemeral (only visible to invoker).
        ephemeral: bool,
    },
    /// Bot command response timed out
    CommandResponseTimeout {
        /// Interaction ID.
        interaction_id: Uuid,
        /// Command name that timed out.
        command_name: String,
        /// Channel where command was invoked.
        channel_id: Uuid,
    },
```

**Step 2: Verify it compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo check`

Expected: PASS (new enum variants don't break existing match arms since ServerEvent uses serde tag dispatch, not exhaustive matches)

**Step 3: Commit**

```
feat(ws): add CommandResponse and CommandResponseTimeout server events

New WebSocket event types for delivering bot command responses to
invoking users. Non-exhaustive enum addition, no breaking changes.
```

---

## Task 5: Implement response delivery relay in bot gateway

**Files:**
- Modify: `server/src/ws/bot_gateway.rs:395-479` (extend CommandResponse handler)

This is the critical fix. When a bot sends a `command_response`, the gateway already stores it in Redis and publishes to `interaction:{id}`. Now we also need to:

1. Look up the interaction context (invoker user_id, channel_id, command_name)
2. For non-ephemeral: create a real message and broadcast to the channel
3. For ephemeral: broadcast to the invoker's user channel only

**Step 1: Store interaction context at invocation time**

First, modify `server/src/chat/messages.rs`. After storing the owner key (line ~535), also store interaction context:

```rust
                        // Store interaction context for response delivery
                        let context_key = format!("interaction:{interaction_id}:context");
                        let context_data = serde_json::json!({
                            "user_id": auth_user.id,
                            "channel_id": channel_id,
                            "guild_id": guild_id,
                            "command_name": command_name,
                        });
                        routing_redis
                            .set::<(), _, _>(
                                &context_key,
                                context_data.to_string(),
                                Some(fred::types::Expiration::EX(300)),
                                None,
                                false,
                            )
                            .await
                            .map_err(|e| {
                                warn!(error = %e, "Failed to store interaction context");
                                MessageError::Validation(
                                    "Bot command routing unavailable".to_string(),
                                )
                            })?;
```

**Step 2: Extend the bot gateway CommandResponse handler**

In `server/src/ws/bot_gateway.rs`, after the existing publish to `interaction:{id}` (line ~476), add response delivery:

```rust
            // Deliver response to invoking user
            let context_key = format!("interaction:{interaction_id}:context");
            if let Ok(Some(context_str)) = state
                .redis
                .get::<Option<String>, _>(&context_key)
                .await
            {
                if let Ok(context) = serde_json::from_str::<serde_json::Value>(&context_str) {
                    let invoker_id = context["user_id"].as_str()
                        .and_then(|s| Uuid::parse_str(s).ok());
                    let channel_id = context["channel_id"].as_str()
                        .and_then(|s| Uuid::parse_str(s).ok());
                    let guild_id = context["guild_id"].as_str()
                        .and_then(|s| Uuid::parse_str(s).ok());
                    let command_name = context["command_name"].as_str()
                        .unwrap_or("unknown").to_string();

                    // Look up bot display name
                    let bot_name = sqlx::query_scalar!(
                        "SELECT display_name FROM users WHERE id = $1",
                        bot_user_id,
                    )
                    .fetch_optional(&state.db)
                    .await
                    .ok()
                    .flatten()
                    .flatten()
                    .unwrap_or_else(|| "Bot".to_string());

                    if let (Some(invoker_id), Some(channel_id)) = (invoker_id, channel_id) {
                        if ephemeral {
                            // Ephemeral: deliver only to invoking user
                            let event = crate::ws::ServerEvent::CommandResponse {
                                interaction_id,
                                content: content.clone(),
                                command_name,
                                bot_name,
                                channel_id,
                                ephemeral: true,
                            };
                            if let Err(e) = crate::ws::broadcast_to_user(
                                &state.redis, invoker_id, &event,
                            ).await {
                                warn!(error = %e, "Failed to deliver ephemeral command response");
                            }
                        } else {
                            // Non-ephemeral: create a real message from the bot
                            let message = sqlx::query!(
                                r#"
                                INSERT INTO messages (channel_id, user_id, content)
                                VALUES ($1, $2, $3)
                                RETURNING id, created_at
                                "#,
                                channel_id,
                                bot_user_id,
                                content,
                            )
                            .fetch_one(&state.db)
                            .await;

                            match message {
                                Ok(msg) => {
                                    // Broadcast as a normal new message to the channel
                                    let author = sqlx::query!(
                                        "SELECT username, display_name, avatar_url, status FROM users WHERE id = $1",
                                        bot_user_id,
                                    )
                                    .fetch_optional(&state.db)
                                    .await
                                    .ok()
                                    .flatten();

                                    let author_data = author.map(|a| serde_json::json!({
                                        "id": bot_user_id,
                                        "username": a.username,
                                        "display_name": a.display_name,
                                        "avatar_url": a.avatar_url,
                                        "status": a.status,
                                    })).unwrap_or_else(|| serde_json::json!({
                                        "id": bot_user_id,
                                        "username": "bot",
                                        "display_name": bot_name,
                                        "avatar_url": null,
                                        "status": "online",
                                    }));

                                    let message_event = crate::ws::ServerEvent::MessageNew {
                                        channel_id,
                                        message: serde_json::json!({
                                            "id": msg.id,
                                            "channel_id": channel_id,
                                            "author": author_data,
                                            "content": content,
                                            "encrypted": false,
                                            "attachments": [],
                                            "reply_to": null,
                                            "parent_id": null,
                                            "thread_reply_count": 0,
                                            "thread_last_reply_at": null,
                                            "edited_at": null,
                                            "created_at": msg.created_at.to_rfc3339(),
                                            "mention_type": null,
                                            "reactions": null,
                                            "thread_info": null,
                                        }),
                                    };

                                    if let Some(gid) = guild_id {
                                        let _ = state.redis
                                            .publish::<(), _, _>(
                                                format!("guild:{gid}"),
                                                serde_json::to_string(&message_event)
                                                    .unwrap_or_default(),
                                            )
                                            .await;
                                    }
                                    let _ = crate::ws::broadcast_to_channel(
                                        &state.redis, channel_id, &message_event,
                                    ).await;
                                }
                                Err(e) => {
                                    warn!(error = %e, "Failed to persist non-ephemeral command response");
                                }
                            }
                        }
                    }
                }
            }
```

**Step 3: Verify it compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo check`

**Step 4: Commit**

```
feat(api): implement command response delivery to invoking user

Stores interaction context (invoker, channel, command name) at
invocation time. Bot gateway now relays responses: non-ephemeral
become persisted messages broadcast to channel, ephemeral delivered
only to invoker via user WebSocket channel.
```

---

## Task 6: Add response timeout relay

**Files:**
- Modify: `server/src/chat/messages.rs:492-576` (spawn timeout task after invocation)

**Step 1: Add timeout task**

After publishing the command invocation (after line ~545 in messages.rs), spawn a background timeout task:

```rust
                        // Spawn timeout relay: notify user if bot doesn't respond in 30s
                        {
                            let redis_url = state.config.redis_url.clone();
                            let invoker_id = auth_user.id;
                            let cmd_name = command_name.clone();
                            let ch_id = channel_id;
                            let iid = interaction_id;
                            tokio::spawn(async move {
                                tokio::time::sleep(std::time::Duration::from_secs(30)).await;
                                // Check if response was already delivered
                                if let Ok(timeout_redis) = db::create_redis_client(&redis_url).await {
                                    let response_key = format!("interaction:{iid}:response");
                                    let exists: bool = timeout_redis
                                        .exists(&response_key)
                                        .await
                                        .unwrap_or(false);
                                    if !exists {
                                        let event = crate::ws::ServerEvent::CommandResponseTimeout {
                                            interaction_id: iid,
                                            command_name: cmd_name,
                                            channel_id: ch_id,
                                        };
                                        let _ = crate::ws::broadcast_to_user(
                                            &timeout_redis, invoker_id, &event,
                                        ).await;
                                    }
                                }
                            });
                        }
```

**Step 2: Verify it compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo check`

**Step 3: Commit**

```
feat(api): add 30-second command response timeout notification

Spawns background task that checks if bot responded after 30 seconds.
Sends CommandResponseTimeout event to invoker if no response found.
```

---

## Task 7: Improve ambiguity error message

**Files:**
- Modify: `server/src/chat/messages.rs:462-472` (include bot names in error)

**Step 1: Update the ambiguity error to include bot names**

Replace the ambiguity check block (lines 462-472):

```rust
                if let Some(command) = commands.first() {
                    let same_priority = commands
                        .iter()
                        .filter(|c| c.guild_scoped == command.guild_scoped)
                        .collect::<Vec<_>>();

                    if same_priority.len() > 1 {
                        // Look up bot names for the error message
                        let bot_ids: Vec<Uuid> = same_priority
                            .iter()
                            .filter_map(|c| c.bot_user_id)
                            .collect();
                        let bot_names: Vec<String> = sqlx::query_scalar!(
                            "SELECT COALESCE(display_name, username) FROM users WHERE id = ANY($1)",
                            &bot_ids,
                        )
                        .fetch_all(&state.db)
                        .await
                        .unwrap_or_default()
                        .into_iter()
                        .flatten()
                        .collect();

                        let names = if bot_names.is_empty() {
                            "multiple bots".to_string()
                        } else {
                            bot_names.join(", ")
                        };
                        return Err(MessageError::Validation(
                            format!("Command '/{command_name}' is ambiguous: provided by {names}"),
                        ));
                    }
```

**Step 2: Verify it compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo check`

**Step 3: Commit**

```
fix(api): include bot names in ambiguous command error message

Error message now shows which bots conflict, e.g. "Command '/ping'
is ambiguous: provided by PingBot, UtilBot".
```

---

## Task 8: Add structured gateway error events

**Files:**
- Modify: `server/src/ws/bot_gateway.rs` (emit error events for parse/validation failures)

**Step 1: Find and improve error handling in the gateway message loop**

In `bot_gateway.rs`, the main WebSocket message processing loop currently logs and continues on errors. Update to emit structured error events back to the bot.

Search for the match arm handling `Message::Text` in the WebSocket loop. When JSON parse fails or `handle_bot_event` returns an error, send an error event back:

For JSON parse failures, send:
```json
{"type": "error", "code": "invalid_json", "message": "Failed to parse message as JSON"}
```

For `handle_bot_event` errors, send:
```json
{"type": "error", "code": "handler_error", "message": "<error details>"}
```

These should use the existing `BotServerEvent::Error` variant.

**Step 2: Verify it compiles**

Run: `cd /home/detair/GIT/detair/canis/server && cargo check`

**Step 3: Commit**

```
feat(ws): emit structured error events for bot gateway failures

Bot gateway now sends explicit error events for JSON parse failures,
handler errors, and unknown event types instead of silently logging.
```

---

## Task 9: Frontend — update autocomplete for ambiguous commands

**Files:**
- Modify: `client/src/components/messages/AutocompletePopup.tsx:169-194`
- Modify: `client/src/components/messages/MessageInput.tsx:172-181` (allow hyphens)

**Step 1: Fix autocomplete trigger regex**

In `MessageInput.tsx`, update the command match regex (line ~174):

```typescript
    const commandMatch = textBeforeCursor.match(/^\/([a-z0-9_-]*)$/);
```

This adds `-` to the allowed characters (was `\w` which doesn't include hyphens in all contexts).

**Step 2: Update autocomplete to show bot name for ambiguous commands**

In `AutocompletePopup.tsx`, update the `commandItems` memo to differentiate ambiguous entries:

```typescript
const commandItems = createMemo((): PopupListItem[] => {
    if (props.type !== "command") return [];

    const query = props.query.toLowerCase();
    const commands = props.commands ?? [];

    const filtered = commands.filter(c => c.name.toLowerCase().includes(query));

    filtered.sort((a, b) => {
        const aStartsWith = a.name.toLowerCase().startsWith(query);
        const bStartsWith = b.name.toLowerCase().startsWith(query);
        if (aStartsWith && !bStartsWith) return -1;
        if (!aStartsWith && bStartsWith) return 1;
        return 0;
    });

    return filtered.slice(0, 8).map(c => ({
        id: c.is_ambiguous ? `${c.name}:${c.application_id}` : c.name,
        label: `/${c.name}`,
        description: c.is_ambiguous
            ? `${c.description} · ${c.bot_name} (ambiguous)`
            : `${c.description} · ${c.bot_name}`,
        icon: <Terminal class="w-4 h-4 text-text-secondary" />,
    }));
});
```

**Step 3: Fix fetch retry lockout**

In `MessageInput.tsx`, change the command fetch effect to allow retry on failure:

```typescript
createEffect(() => {
    if (autocompleteType() === "command" && !commandsFetched() && props.guildId) {
        setCommandsFetched(true);
        listGuildCommands(props.guildId!).then(setGuildCommands).catch(() => {
            setGuildCommands([]);
            setCommandsFetched(false); // Allow retry on next / keystroke
        });
    }
});
```

**Step 4: Verify frontend builds**

Run: `cd /home/detair/GIT/detair/canis/client && bun run build`

**Step 5: Commit**

```
fix(client): improve slash command autocomplete reliability

- Allow hyphens in autocomplete trigger regex
- Show "(ambiguous)" label when multiple bots provide same command
- Reset fetch state on failure to allow retry
```

---

## Task 10: Frontend — handle command response events

**Files:**
- Modify: `client/src/stores/websocket.ts` (add event handlers)
- Modify: `client/src/lib/types.ts` (add event types if needed)

**Step 1: Add command response handler in WebSocket store**

In `client/src/stores/websocket.ts`, add cases to the main event switch:

```typescript
    case "command_response":
      if (event.ephemeral) {
        // Show ephemeral response as a local-only message
        addEphemeralMessage(event.channel_id, {
          content: event.content,
          bot_name: event.bot_name,
          command_name: event.command_name,
          interaction_id: event.interaction_id,
        });
      }
      // Non-ephemeral responses arrive as normal message_new events
      break;

    case "command_response_timeout":
      addEphemeralMessage(event.channel_id, {
        content: `Command /${event.command_name} timed out — the bot didn't respond.`,
        bot_name: "System",
        command_name: event.command_name,
        interaction_id: event.interaction_id,
      });
      break;
```

Note: `addEphemeralMessage` is a new helper that adds a transient message to the local message list without persisting. It should render with a distinct style (bot badge, "Only you can see this" for ephemeral). The exact implementation depends on how the messages store works — adapt to the existing `addMessage` pattern but skip persistence.

**Step 2: Verify frontend builds**

Run: `cd /home/detair/GIT/detair/canis/client && bun run build`

**Step 3: Commit**

```
feat(client): handle command response and timeout WebSocket events

Ephemeral responses shown as local-only messages with bot badge.
Non-ephemeral arrive as normal message_new events. Timeouts show
user-friendly error message.
```

---

## Task 11: Built-in /ping command

**Files:**
- Modify: `server/src/chat/messages.rs:434-440` (add built-in /ping before bot routing)

**Step 1: Write the failing test**

Add to `server/tests/bot_ecosystem_test.rs`:

```rust
#[sqlx::test]
async fn test_builtin_ping_command(pool: PgPool) {
    let state = setup_test_state(pool).await;
    let (jwt, _) = create_test_user(&state).await;
    let guild_id = create_test_guild(&state, &jwt).await;
    let channel_id = create_test_channel(&state, guild_id).await;

    let res = state.client
        .post(&format!("/api/messages/channel/{channel_id}"))
        .bearer_auth(&jwt)
        .json(&serde_json::json!({"content": "/ping"}))
        .send()
        .await
        .unwrap();

    // Built-in /ping returns 200 (not 202) with Pong! content
    assert_eq!(res.status(), 200);
    let body: serde_json::Value = res.json().await.unwrap();
    assert!(body["content"].as_str().unwrap().starts_with("Pong!"));
}
```

**Step 2: Run test to verify it fails**

Expected: FAIL (no built-in /ping handling exists)

**Step 3: Implement built-in /ping**

In `server/src/chat/messages.rs`, add before the bot routing block (before line 441):

```rust
                // Built-in /ping: responds directly without bot routing
                if command_name == "ping" {
                    let start = std::time::Instant::now();
                    let author = db::find_user_by_id(&state.db, auth_user.id)
                        .await?
                        .map(AuthorProfile::from)
                        .unwrap_or_else(|| AuthorProfile {
                            id: auth_user.id,
                            username: "unknown".to_string(),
                            display_name: "Unknown User".to_string(),
                            avatar_url: None,
                            status: "offline".to_string(),
                        });
                    let latency_ms = start.elapsed().as_millis();
                    let content = format!("Pong! (latency: {latency_ms}ms)");

                    // Persist as a system message
                    let msg = sqlx::query!(
                        r#"
                        INSERT INTO messages (channel_id, user_id, content)
                        VALUES ($1, $2, $3)
                        RETURNING id, created_at
                        "#,
                        channel_id,
                        auth_user.id,
                        content,
                    )
                    .fetch_one(&state.db)
                    .await
                    .map_err(MessageError::Database)?;

                    let response = MessageResponse {
                        id: msg.id,
                        channel_id,
                        author,
                        content,
                        encrypted: false,
                        attachments: vec![],
                        reply_to: None,
                        parent_id: None,
                        thread_reply_count: 0,
                        thread_last_reply_at: None,
                        edited_at: None,
                        created_at: msg.created_at.and_utc(),
                        mention_type: None,
                        reactions: None,
                        thread_info: None,
                    };

                    return Ok((StatusCode::OK, Json(response)));
                }
```

**Step 4: Run test to verify it passes**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test --test bot_ecosystem_test test_builtin_ping -- --nocapture`

**Step 5: Commit**

```
feat(api): add built-in /ping command for smoke testing

Server handles /ping natively in any guild channel without bot
installation. Returns "Pong!" with server-side latency measurement.
Persisted as a regular message for visibility.
```

---

## Task 12: Example bot script

**Files:**
- Create: `docs/examples/ping-bot.py`
- Modify: `docs/development/bot-system.md` (link to example)

**Step 1: Write the example bot**

Create `docs/examples/ping-bot.py` with a complete, self-contained Python bot that demonstrates the full lifecycle: create app, register /ping command, connect to gateway, handle command_invoked events, send command_response.

Reference the existing pseudocode in `docs/development/bot-system.md` but make it a fully runnable script with `websocket-client` and `requests` as dependencies.

**Step 2: Add reference in bot-system.md**

Add a "Getting Started Example" section pointing to `docs/examples/ping-bot.py`.

**Step 3: Commit**

```
docs(api): add example ping bot script

Standalone Python bot demonstrating full bot lifecycle: app creation,
command registration, gateway connection, and command response handling.
```

---

## Task 13: Integration tests for response delivery

**Files:**
- Modify: `server/tests/bot_ecosystem_test.rs`

**Step 1: Write tests for the full response round-trip**

Add integration tests that:
1. Invoke a slash command
2. Simulate bot response via Redis (write to `interaction:{id}:response` and publish)
3. Verify the response is stored and published correctly
4. Test ephemeral flag behavior
5. Test the ambiguity error includes bot names

These tests build on the existing invocation test pattern but extend to cover the full response path.

**Step 2: Run all bot ecosystem tests**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test --test bot_ecosystem_test -- --nocapture`

Expected: All tests PASS

**Step 3: Commit**

```
test(api): add integration tests for command response delivery

Tests cover: non-ephemeral response persistence, ephemeral delivery,
ambiguity error with bot names, built-in /ping, batch duplicate
rejection, multi-provider listing.
```

---

## Task 14: Run full quality gates

**Files:** None (verification only)

**Step 1: Run all server tests**

Run: `cd /home/detair/GIT/detair/canis/server && cargo test`

**Step 2: Run lints**

Run: `cd /home/detair/GIT/detair/canis/server && cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings`

**Step 3: Run frontend tests and build**

Run: `cd /home/detair/GIT/detair/canis/client && bun run test:run && bun run build`

**Step 4: Run license check**

Run: `cd /home/detair/GIT/detair/canis/server && cargo deny check licenses`

**Step 5: Fix any issues and commit**

---

## Task 15: Update roadmap and changelog

**Files:**
- Modify: `docs/project/roadmap.md` (mark item complete, update percentages)
- Modify: `CHANGELOG.md` (add entry under Unreleased)

**Step 1: Update roadmap**

Mark the "Slash Command Reliability & /ping Reference Command" checklist item as `[x]` complete. Update the Phase 5 completion percentage.

**Step 2: Update CHANGELOG.md**

Add under `[Unreleased]` > `### Added` / `### Fixed`:

```markdown
### Added
- Built-in `/ping` command for smoke testing in any guild channel
- Command response delivery: bot responses now visible to users via WebSocket relay
- Ephemeral command responses (visible only to invoker)
- 30-second timeout notification when bots don't respond
- Structured gateway error events for bot developers
- Example Python bot script (`docs/examples/ping-bot.py`)

### Fixed
- Slash command autocomplete now allows hyphens in command names
- Guild command listing shows all providers instead of hiding duplicates
- Ambiguity errors now include bot names for disambiguation
- Command fetch retry no longer locks out on failure
```

**Step 3: Commit**

```
docs(api): update roadmap and changelog for slash command reliability
```

---

## Verification Commands

After each task:
```bash
cd /home/detair/GIT/detair/canis/server && cargo check
```

Full quality gates (Task 14):
```bash
cd /home/detair/GIT/detair/canis/server && cargo test
cd /home/detair/GIT/detair/canis/server && cargo fmt --check && cargo clippy --all-targets --all-features -- -D warnings
cd /home/detair/GIT/detair/canis/server && cargo deny check licenses
cd /home/detair/GIT/detair/canis/client && bun run test:run && bun run build
```
