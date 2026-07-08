// Announcement channels stress scenario.
//
// Drives the announcement hot paths under load:
//   - POST /api/messages/channel/{source}   (publish → spawns crosspost fan-out)
//   - GET  /api/channels/{source}/followers  (source-side follower list)
//
// A follow is created once in setup() so each publish fans out to one target.
// The fan-out is async (spawned), so the publish latency measured here is the
// publisher-facing cost; watch the admin Command Center for fan-out load.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/announcements.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/announcements.js

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
    JSON.stringify({ name: `ann-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  const source = http.post(
    `${BASE_URL}/api/channels`,
    JSON.stringify({ name: "news", channel_type: "announcement", guild_id: guildId }),
    jsonHeaders,
  );
  check(source, { "setup: announcement channel": (r) => r.status === 200 || r.status === 201 });
  const sourceId = source.json("id");

  const target = http.post(
    `${BASE_URL}/api/channels`,
    JSON.stringify({ name: "inbox", channel_type: "text", guild_id: guildId }),
    jsonHeaders,
  );
  const targetId = target.json("id");

  // Follow so each publish fans out.
  const follow = http.post(
    `${BASE_URL}/api/channels/${targetId}/follow`,
    JSON.stringify({ source_channel_id: sourceId }),
    jsonHeaders,
  );
  check(follow, { "setup: follow created": (r) => r.status === 200 || r.status === 201 });

  return { token, guildId, sourceId };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Write: publish an announcement (owner has SEND_ANNOUNCEMENTS; triggers fan-out).
  const publish = recordResponse(
    http.post(
      `${BASE_URL}/api/messages/channel/${data.sourceId}`,
      JSON.stringify({ content: `news ${Date.now()}`, encrypted: false }),
      jsonHeaders,
    ),
  );
  check(publish, { "publish: ok or rate-limited": (r) => r.status < 500 });

  // Read: followers list.
  const followers = recordResponse(
    http.get(`${BASE_URL}/api/channels/${data.sourceId}/followers`, h),
  );
  check(followers, { "followers: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
