// Reaction-roles stress scenario.
//
// Drives the feature's hot paths under concurrent load:
//   - GET  /api/guilds/{id}/reaction-roles          (list bindings, read)
//   - PUT  /api/channels/{cid}/messages/{mid}/reactions   (react → grant role)
//   - DELETE .../reactions/{emoji}                  (un-react → revoke role)
//
// The PUT/DELETE exercise the transactional reaction-role hook (binding lookup,
// role grant/revoke, MemberRolesUpdated broadcast). setup() builds all fixtures
// once (guild, channel, message, role, binding) so per-VU iterations only touch
// the endpoints under test.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/reaction-roles.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/reaction-roles.js
//
// An ASCII token ("rr") is used as the "emoji" so the DELETE path needs no
// URL-encoding; the server accepts any string ≤64 chars as a reaction key.

import http from "k6/http";
import { check, sleep } from "k6";
import { BASE_URL, baseOptions } from "../lib/config.js";
import { authenticate, authHeaders } from "../lib/auth.js";
import { recordResponse } from "../lib/metrics.js";

export const options = baseOptions;

const EMOJI = "rr";

export function setup() {
  const token = authenticate();
  const h = authHeaders(token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  const guild = http.post(
    `${BASE_URL}/api/guilds`,
    JSON.stringify({ name: `rr-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  const channel = http.post(
    `${BASE_URL}/api/channels`,
    JSON.stringify({ name: "rr-stress", channel_type: "text", guild_id: guildId }),
    jsonHeaders,
  );
  check(channel, { "setup: channel created": (r) => r.status === 200 || r.status === 201 });
  const channelId = channel.json("id");

  const message = http.post(
    `${BASE_URL}/api/messages/channel/${channelId}`,
    JSON.stringify({ content: "React to get the role" }),
    jsonHeaders,
  );
  check(message, { "setup: message created": (r) => r.status === 200 || r.status === 201 });
  const messageId = message.json("id");

  // A safe (no-permission) role, bindable by the owner (who bypasses hierarchy).
  const role = http.post(
    `${BASE_URL}/api/guilds/${guildId}/roles`,
    JSON.stringify({ name: "rr-stress-role", permissions: 0 }),
    jsonHeaders,
  );
  check(role, { "setup: role created": (r) => r.status === 200 || r.status === 201 });
  const roleId = role.json("id");

  const binding = http.post(
    `${BASE_URL}/api/guilds/${guildId}/reaction-roles`,
    JSON.stringify({
      channel_id: channelId,
      message_id: messageId,
      emoji: EMOJI,
      role_id: roleId,
      mode: "toggle",
    }),
    jsonHeaders,
  );
  check(binding, { "setup: binding created": (r) => r.status === 200 || r.status === 201 });

  return { token, guildId, channelId, messageId };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Read: list bindings for the guild.
  const list = recordResponse(
    http.get(`${BASE_URL}/api/guilds/${data.guildId}/reaction-roles`, h),
  );
  check(list, { "list: ok or rate-limited": (r) => r.status < 500 });

  // Write: react → grant the role (runs the transactional hook).
  const react = recordResponse(
    http.put(
      `${BASE_URL}/api/channels/${data.channelId}/messages/${data.messageId}/reactions`,
      JSON.stringify({ emoji: EMOJI }),
      jsonHeaders,
    ),
  );
  check(react, { "react: ok or rate-limited": (r) => r.status < 500 });

  // Write: un-react → revoke the role.
  const unreact = recordResponse(
    http.del(
      `${BASE_URL}/api/channels/${data.channelId}/messages/${data.messageId}/reactions/${EMOJI}`,
      null,
      h,
    ),
  );
  check(unreact, { "unreact: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  // Guild delete cascades to channel, message, role, and bindings.
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
