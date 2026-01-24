# Design: WASM-based Server Plugins (Phase 5)

## 1. Overview
Integrate `wasmtime` to allow safe, sandboxed execution of user-uploaded bot logic.

## 2. Database Schema

### Migration SQL
File: `server/migrations/20260125000001_add_plugins.sql`

```sql
CREATE TABLE plugins (
    id UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    guild_id UUID NOT NULL REFERENCES guilds(id) ON DELETE CASCADE,
    name VARCHAR(64) NOT NULL,
    description TEXT,
    wasm_blob BYTEA NOT NULL, -- The compiled .wasm binary
    config JSONB NOT NULL DEFAULT '{}', -- Env vars / settings for the bot
    enabled BOOLEAN NOT NULL DEFAULT TRUE,
    permissions JSONB NOT NULL DEFAULT '[]', -- List of capabilities: ["SEND_MESSAGES", "KICK"]
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX idx_plugins_guild ON plugins(guild_id);
```

## 3. Backend Implementation (Rust)

### 3.1. Dependencies
Add to `server/Cargo.toml`:
```toml
wasmtime = "16.0"
wasi-common = "16.0"
```

### 3.2. Plugin Engine (`server/src/plugins/engine.rs`)
We need a structure to hold the `Engine` (compilation cache).

```rust
pub struct PluginSystem {
    engine: wasmtime::Engine,
    // Cache compiled modules: GuildID -> Module
    cache: DashMap<Uuid, wasmtime::Module>, 
}
```

### 3.3. Host Interface (WIT)
Define `plugin.wit`:
```wit
interface host-api {
    log: func(msg: string)
    send_message: func(channel_id: string, content: string) -> result<string, string>
}

world plugin {
    import host-api
    export on_message: func(msg_json: string)
}
```

### 3.4. Execution Flow
In `server/src/chat/messages.rs`, inside `create_message` handler:
1.  After inserting message to DB.
2.  `tokio::spawn(async move { plugin_system.trigger_on_message(guild_id, message).await })`
3.  The system loads the WASM for that guild, creates a `Store`, links imports, and calls `on_message`.

## 4. Security Model
*   **Fuel/Metering:** Configure `Config::consume_fuel(true)` to limit CPU usage (infinite loops).
*   **Memory:** `Store::new` with `ResourceLimiter` (max 16MB linear memory).
*   **Permissions:** Before host function `send_message` runs, check if `plugin.permissions` contains `"SEND_MESSAGES"`.

## 5. Step-by-Step Plan
1.  **DB:** Create `plugins` table migration.
2.  **Server:** Add `wasmtime` dependencies.
3.  **Server:** Create `server/src/plugins/` module.
4.  **Server:** Implement `HostFunctions` struct that bridges WASM calls to internal service calls.
5.  **Server:** Hook `trigger_on_message` into the message creation flow.
6.  **Client:** Admin UI to upload `.wasm` file and configure JSON settings.