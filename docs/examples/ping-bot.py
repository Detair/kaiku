#!/usr/bin/env python3
"""Example bot that responds to /ping with Pong! and latency.

Prerequisites:
    pip install requests websocket-client

Usage:
    1. Set SERVER_URL, USER_JWT below (or via environment variables).
    2. Run: python3 ping-bot.py
    3. Install the bot in a guild, then type /ping in any channel.
"""

import json
import os
import sys
import time

import requests
import websocket

SERVER_URL = os.environ.get("CANIS_SERVER_URL", "https://localhost:3000")
USER_JWT = os.environ.get("CANIS_USER_JWT", "")

if not USER_JWT:
    print("Set CANIS_USER_JWT to your developer JWT token.")
    sys.exit(1)

headers = {"Authorization": f"Bearer {USER_JWT}", "Content-Type": "application/json"}


def api(method, path, json_data=None):
    url = f"{SERVER_URL}{path}"
    resp = getattr(requests, method)(url, headers=headers, json=json_data, verify=False)
    resp.raise_for_status()
    return resp.json() if resp.content else None


# 1. Create application
print("Creating application...")
app = api("post", "/api/applications", {"name": "PingBot", "description": "Responds to /ping"})
app_id = app["id"]
print(f"  Application ID: {app_id}")

# 2. Create bot user (token shown ONCE)
print("Creating bot user...")
bot = api("post", f"/api/applications/{app_id}/bot")
bot_token = bot["token"]
bot_user_id = bot["bot_user_id"]
print(f"  Bot user ID: {bot_user_id}")
print(f"  Token: {bot_token} (save this!)")

# 3. Register /ping command
print("Registering /ping command...")
api("put", f"/api/applications/{app_id}/commands", {
    "commands": [{"name": "ping", "description": "Check bot latency", "options": []}]
})
print("  Registered.")

# 4. Connect to bot gateway
ws_url = SERVER_URL.replace("https", "wss").replace("http", "ws")
print(f"Connecting to gateway: {ws_url}/api/gateway/bot")

ws = websocket.create_connection(
    f"{ws_url}/api/gateway/bot",
    header={"Authorization": f"Bot {bot_token}"},
    sslopt={"cert_reqs": 0},
)
print("Connected! Waiting for events...\n")

# 5. Event loop
try:
    while True:
        raw = ws.recv()
        event = json.loads(raw)
        event_type = event.get("type", "unknown")

        if event_type == "command_invoked":
            cmd = event["command_name"]
            iid = event["interaction_id"]
            user = event["user_id"]
            print(f"[command] /{cmd} from {user} (interaction: {iid})")

            if cmd == "ping":
                start = time.monotonic()
                ws.send(json.dumps({
                    "type": "command_response",
                    "interaction_id": iid,
                    "content": f"Pong! (bot latency: {int((time.monotonic() - start) * 1000)}ms)",
                    "ephemeral": False,
                }))
                print(f"  Responded with Pong!")

        elif event_type == "message_created":
            print(f"[message] {event['user_id']}: {event['content'][:80]}")

        elif event_type == "guild_joined":
            print(f"[guild] Joined: {event['guild_name']}")

        elif event_type == "guild_left":
            print(f"[guild] Left: {event['guild_id']}")

        elif event_type == "error":
            print(f"[error] {event['code']}: {event['message']}")

        else:
            print(f"[{event_type}] {json.dumps(event)[:120]}")

except KeyboardInterrupt:
    print("\nShutting down...")
finally:
    ws.close()

# Cleanup: delete application (cascades to bot user and commands)
print("Cleaning up...")
try:
    api("delete", f"/api/applications/{app_id}")
    print("  Application deleted.")
except Exception:
    print(f"  Cleanup failed. Delete manually: DELETE /api/applications/{app_id}")
