# Bot System

Bots are automated user accounts that connect to guilds via the Bot Gateway WebSocket.
They can receive events, respond to slash commands, and send messages to channels.

```
                          Capabilities
  +--------------------------------------------------------+
  | Slash commands   | Register custom /commands            |
  | Messages         | Send and receive text messages       |
  | Guild events     | Detect join/leave events             |
  | Ephemeral reply  | Private command responses            |
  +--------------------------------------------------------+
```

## For Guild Admins

### Installing a Bot

Guild members with the `MANAGE_GUILD` permission can install bots.

1. Obtain the bot's **bot user ID** from the bot developer.
2. Call the installation endpoint:

```http
POST /api/guilds/{guild_id}/bots/{bot_user_id}/add
Authorization: Bearer <your_jwt>
```

- The bot must be marked `public`, or you must be the application owner.
- Duplicate installations are silently ignored (`ON CONFLICT DO NOTHING`).

### Managing Commands

Once a bot is installed, its registered slash commands become available in the guild.
Guild admins can view installed bots and their commands via the **Slash Commands** page
in the client settings UI.

### Removing a Bot

Delete the installation record from `guild_bot_installations` to remove a bot.
This does not delete the bot user or its commands.

## Getting Started (Bot Developers)

### Step-by-step Walkthrough

1. **Create an application** — register your bot's identity.
2. **Create a bot user** — generates a user account and auth token.
3. **Save the token** — it is only shown once.
4. **Register commands** — define slash commands your bot handles.
5. **Connect to the gateway** — open a WebSocket to receive events.
6. **Handle events** — process commands and messages.

```
  Developer                  Server                     Guild
  ---------                  ------                     -----
      |                        |                          |
      |-- POST /applications ->|                          |
      |<- { id, name }        |                          |
      |                        |                          |
      |-- POST /apps/{id}/bot>|                          |
      |<- { token, bot_id }   |  (save token!)           |
      |                        |                          |
      |-- PUT /apps/{id}/cmds>|                          |
      |<- [ commands ]         |                          |
      |                        |                          |
      |-- WS /api/gateway/bot |                          |
      |   Auth: Bot <token>    |                          |
      |<======= events ======>|                          |
      |                        |                          |
      |                        |<-- admin installs bot ---|
      |<- guild_joined event   |                          |
```

## Authentication

### Token Format

```
{bot_user_id}.{secret}
```

- `bot_user_id` — UUID of the bot's user account (used for indexed lookup).
- `secret` — random UUID generated on creation.

### Authorization Header

```
Authorization: Bot {bot_user_id}.{secret}
```

The `Bot ` prefix (with space) distinguishes bot tokens from user JWTs.

### Security Notes

- The token is hashed with **Argon2id** (CSPRNG salt) before storage.
- The plaintext token is returned **once** on creation or reset; the server never stores it.
- Token reset invalidates the previous token immediately.
- Bot management endpoints (application CRUD, commands) use **user JWT auth** (`Bearer`),
  not bot tokens.

## REST API Reference

All endpoints return JSON. UUIDs are formatted as lowercase hyphenated strings.

### Applications

#### Create Application

```http
POST /api/applications
Authorization: Bearer <jwt>
Content-Type: application/json

{
  "name": "My Bot",
  "description": "Does cool things"
}
```

**Response** `201 Created`:

```json
{
  "id": "uuid",
  "name": "My Bot",
  "description": "Does cool things",
  "bot_user_id": null,
  "public": true,
  "created_at": "2026-01-15T12:00:00Z"
}
```

#### List Applications

```http
GET /api/applications
Authorization: Bearer <jwt>
```

**Response** `200 OK`: array of `ApplicationResponse`.

#### Get Application

```http
GET /api/applications/{id}
Authorization: Bearer <jwt>
```

**Response** `200 OK`: single `ApplicationResponse`.

#### Delete Application

```http
DELETE /api/applications/{id}
Authorization: Bearer <jwt>
```

**Response** `204 No Content`.

Deleting an application cascades to its bot user, commands, and guild installations.

### Bot User & Token

#### Create Bot User

```http
POST /api/applications/{id}/bot
Authorization: Bearer <jwt>
```

**Response** `201 Created`:

```json
{
  "token": "bot_user_id.secret",
  "bot_user_id": "uuid"
}
```

- Returns `409 Conflict` if a bot user already exists for this application.
- The bot user gets username `bot_{app_id_prefix}` and display name `{app_name} (Bot)`.

#### Reset Bot Token

```http
POST /api/applications/{id}/reset-token
Authorization: Bearer <jwt>
```

**Response** `200 OK`:

```json
{
  "token": "bot_user_id.new_secret",
  "bot_user_id": "uuid"
}
```

The old token is immediately invalidated.

### Slash Commands

Commands can be **global** (omit `guild_id`) or **guild-scoped** (provide `?guild_id=`).

#### Register Commands (Bulk Replace)

Replaces all commands for the given scope (global or guild).

```http
PUT /api/applications/{id}/commands?guild_id={guild_id}
Authorization: Bearer <jwt>
Content-Type: application/json

{
  "commands": [
    {
      "name": "ping",
      "description": "Check bot latency",
      "options": []
    },
    {
      "name": "greet",
      "description": "Greet a user",
      "options": [
        {
          "name": "user",
          "description": "User to greet",
          "type": "user",
          "required": true
        }
      ]
    }
  ]
}
```

**Response** `200 OK`: array of `CommandResponse`.

```json
[
  {
    "id": "uuid",
    "application_id": "uuid",
    "guild_id": null,
    "name": "ping",
    "description": "Check bot latency",
    "options": [],
    "created_at": "2026-01-15T12:00:00Z"
  }
]
```

#### List Commands

```http
GET /api/applications/{id}/commands?guild_id={guild_id}
Authorization: Bearer <jwt>
```

**Response** `200 OK`: array of `CommandResponse`.

#### Delete Single Command

```http
DELETE /api/applications/{id}/commands/{command_id}
Authorization: Bearer <jwt>
```

**Response** `204 No Content`.

#### Delete All Commands (Scope)

```http
DELETE /api/applications/{id}/commands?guild_id={guild_id}
Authorization: Bearer <jwt>
```

**Response** `204 No Content`.

Omit `guild_id` to delete all global commands.

### Command Option Types

| Type      | Description         |
|-----------|---------------------|
| `string`  | Text input          |
| `integer` | Whole number        |
| `boolean` | True/false toggle   |
| `user`    | User mention        |
| `channel` | Channel mention     |
| `role`    | Role mention        |

### Guild Installation

#### Install Bot

Requires `MANAGE_GUILD` permission.

```http
POST /api/guilds/{guild_id}/bots/{bot_user_id}/add
Authorization: Bearer <jwt>
```

**Response** `204 No Content`.

- The bot must be `public`, or the caller must own the application.
- Duplicate installations are silently ignored.

## Bot Gateway WebSocket

### Connection

```
GET /api/gateway/bot
Authorization: Bot {token}
```

Upgrades to WebSocket on successful authentication. The server:

1. Parses the `bot_user_id` from the token.
2. Looks up `bot_applications` by `bot_user_id` (indexed query).
3. Verifies the token hash with Argon2id (constant-time).
4. Subscribes to the Redis pubsub channel `bot:{bot_user_id}`.

### Inbound Events (Server to Bot)

All events use `{"type": "event_name", ...}` envelope with `snake_case` tags.

#### `command_invoked`

A user invoked one of the bot's slash commands.

```json
{
  "type": "command_invoked",
  "interaction_id": "uuid",
  "command_name": "ping",
  "guild_id": "uuid",
  "channel_id": "uuid",
  "user_id": "uuid",
  "options": {}
}
```

- `interaction_id` is used to send a `command_response` back.
- `guild_id` is `null` for DM commands.
- `options` is a JSON object with the command arguments.

#### `message_created`

A message was posted in a channel the bot has access to.

```json
{
  "type": "message_created",
  "message_id": "uuid",
  "channel_id": "uuid",
  "guild_id": "uuid",
  "user_id": "uuid",
  "content": "Hello world"
}
```

#### `guild_joined`

The bot was installed in a guild.

```json
{
  "type": "guild_joined",
  "guild_id": "uuid",
  "guild_name": "My Server"
}
```

#### `guild_left`

The bot was removed from a guild.

```json
{
  "type": "guild_left",
  "guild_id": "uuid"
}
```

#### `error`

Server-side error related to a bot action.

```json
{
  "type": "error",
  "code": "rate_limited",
  "message": "Rate limit exceeded; retry after 5 seconds"
}
```

### Outbound Events (Bot to Server)

#### `message_create`

Send a message to a channel the bot is a member of.

```json
{
  "type": "message_create",
  "channel_id": "uuid",
  "content": "Hello from bot!"
}
```

- Content must be 1-4000 characters.
- Bot must be a member of the target channel.
- Messages are broadcast to all channel subscribers.

#### `command_response`

Respond to a slash command invocation.

```json
{
  "type": "command_response",
  "interaction_id": "uuid",
  "content": "Pong! Latency: 12ms",
  "ephemeral": false
}
```

- `interaction_id` must match a `command_invoked` event the bot received.
- `ephemeral: true` makes the response visible only to the invoking user.
- Content must be 1-4000 characters.
- Responses are stored in Redis with a 5-minute TTL.
- The bot must own the interaction (verified via `interaction:{id}:owner` key).
- **Single-response only:** Each `interaction_id` accepts exactly one `CommandResponse`. Subsequent responses return an error (`"Response already provided for this interaction"`). This is enforced atomically via `SET NX` in Redis.

## Command Invocation Flow

```
  User              Server              Redis              Bot
  ----              ------              -----              ---
    |                  |                  |                  |
    |-- /ping -------->|                  |                  |
    |                  |-- store owner -->|                  |
    |                  |  interaction:    |                  |
    |                  |    {id}:owner    |                  |
    |                  |                  |                  |
    |                  |-- publish ------>|                  |
    |                  |  bot:{bot_id}    |--- event ------->|
    |                  |                  |  command_invoked  |
    |                  |                  |                  |
    |                  |                  |<-- response -----|
    |                  |                  |  command_response |
    |                  |<-- subscribe ----|                  |
    |                  |  interaction:    |                  |
    |                  |    {id}          |                  |
    |<-- response -----|                  |                  |
    |  "Pong!"         |                  |                  |
```

## Validation Rules

| Field                | Constraint                                  |
|----------------------|---------------------------------------------|
| Application name     | 2-100 characters                            |
| Application desc     | Max 1000 characters (optional)              |
| Command name         | 1-32 chars, lowercase `[a-z0-9_-]`          |
| Command description  | 1-100 characters                            |
| Message content      | 1-4000 characters                           |
| Command response     | 1-4000 characters                           |

Command names are validated by `validate_command_name()`: must be non-empty, max 32 chars,
and consist only of ASCII lowercase letters, digits, hyphens, and underscores.

## Rate Limits

Bot gateway events use the `WsMessage` rate limit category:

| Category     | Requests | Window |
|--------------|----------|--------|
| `ws_message` | 60       | 60s    |

Rate limits are applied per bot user (`bot_ws:{bot_user_id}`). When exceeded, the
bot receives an `error` event and must wait for the `retry_after` period.

Rate limits are configurable via the `RATE_LIMIT_WS_MESSAGE` environment variable.

## Complete Example

Pseudocode showing the full bot lifecycle:

```python
import requests
import websocket
import json

SERVER = "https://chat.example.com"
JWT = "your_user_jwt"  # Developer's JWT for setup

# 1. Create application
app = requests.post(f"{SERVER}/api/applications",
    headers={"Authorization": f"Bearer {JWT}"},
    json={"name": "PingBot", "description": "Responds to /ping"}
).json()
app_id = app["id"]

# 2. Create bot user (token shown ONCE)
bot = requests.post(f"{SERVER}/api/applications/{app_id}/bot",
    headers={"Authorization": f"Bearer {JWT}"}
).json()
BOT_TOKEN = bot["token"]  # Save this securely!
bot_user_id = bot["bot_user_id"]

# 3. Register commands
requests.put(f"{SERVER}/api/applications/{app_id}/commands",
    headers={
        "Authorization": f"Bearer {JWT}",
        "Content-Type": "application/json",
    },
    json={"commands": [
        {"name": "ping", "description": "Check bot latency", "options": []},
    ]}
)

# 4. Connect to gateway
ws = websocket.create_connection(
    f"{SERVER.replace('https', 'wss')}/api/gateway/bot",
    header={"Authorization": f"Bot {BOT_TOKEN}"}
)

# 5. Event loop
while True:
    raw = ws.recv()
    event = json.loads(raw)

    if event["type"] == "command_invoked":
        if event["command_name"] == "ping":
            ws.send(json.dumps({
                "type": "command_response",
                "interaction_id": event["interaction_id"],
                "content": "Pong!",
                "ephemeral": False,
            }))

    elif event["type"] == "message_created":
        print(f"Message in {event['channel_id']}: {event['content']}")

    elif event["type"] == "guild_joined":
        print(f"Joined guild: {event['guild_name']}")

    elif event["type"] == "error":
        print(f"Error: {event['message']}")
```

## Runnable Example

A complete, runnable Python bot is available at [`docs/examples/ping-bot.py`](../examples/ping-bot.py).
It demonstrates the full lifecycle: creating an application, registering `/ping`, connecting to the gateway, and responding to commands.

## Database Schema

For reference, the bot ecosystem uses these tables:

| Table                       | Purpose                              |
|-----------------------------|--------------------------------------|
| `bot_applications`          | Application registry (owner, token)  |
| `slash_commands`            | Registered commands per application  |
| `guild_bot_installations`   | Which bots are installed where       |
| `users` (is_bot, bot_owner) | Bot user accounts                    |

See migration `20260202204100_bot_ecosystem.sql` for full schema.
