// Rich-embeds / components stress scenario.
//
// Feature #2 added embed/component JSONB columns to `messages` and threads them
// through message create + every message-list serialization. Embeds/components
// themselves are bot-only (a bot account can't be provisioned over the public
// API), so this scenario stresses the two message hot paths that feature #2
// modified for ALL messages — create and list — to confirm the added
// serialization / JSONB mapping introduces no regression under load.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/rich-embeds.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/rich-embeds.js

import http from "k6/http";
import { check, sleep } from "k6";
import { BASE_URL, baseOptions } from "../lib/config.js";
import { authenticate, authHeaders } from "../lib/auth.js";
import { recordResponse } from "../lib/metrics.js";

export const options = baseOptions;

export function setup() {
  const token = authenticate();
  const h = authHeaders(token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  const guild = http.post(
    `${BASE_URL}/api/guilds`,
    JSON.stringify({ name: `embed-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  const channel = http.post(
    `${BASE_URL}/api/channels`,
    JSON.stringify({ name: "embed-stress", channel_type: "text", guild_id: guildId }),
    jsonHeaders,
  );
  check(channel, { "setup: channel created": (r) => r.status === 200 || r.status === 201 });
  const channelId = channel.json("id");

  return { token, guildId, channelId };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Write: create a message (exercises the create path + embed/component branch).
  const create = recordResponse(
    http.post(
      `${BASE_URL}/api/messages/channel/${data.channelId}`,
      JSON.stringify({ content: `stress ${Date.now()}`, encrypted: false }),
      jsonHeaders,
    ),
  );
  check(create, { "create: ok or rate-limited": (r) => r.status < 500 });

  // Read: list messages (exercises build_message_responses with embed/component
  // JSONB mapping for every row).
  const list = recordResponse(
    http.get(`${BASE_URL}/api/messages/channel/${data.channelId}?limit=50`, h),
  );
  check(list, { "list: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
