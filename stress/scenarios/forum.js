// Forum channels stress scenario.
//
// Drives the two new forum hot paths under load:
//   - POST /api/channels/{id}/posts   (create post: root message + forum_posts
//                                       + tag links in one transaction)
//   - GET  /api/channels/{id}/posts   (list: joins messages for reply_count,
//                                       resolves tag ids per post)
//
// The list path does a per-post tag lookup, so watch for an N+1 (latency
// climbing with post count / VUs) via the admin Command Center.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/forum.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/forum.js

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
    JSON.stringify({ name: `forum-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  const channel = http.post(
    `${BASE_URL}/api/channels`,
    JSON.stringify({ name: "forum-stress", channel_type: "forum", guild_id: guildId }),
    jsonHeaders,
  );
  check(channel, { "setup: forum channel created": (r) => r.status === 200 || r.status === 201 });
  const channelId = channel.json("id");

  return { token, guildId, channelId };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Write: create a forum post (one transaction).
  const create = recordResponse(
    http.post(
      `${BASE_URL}/api/channels/${data.channelId}/posts`,
      JSON.stringify({ title: `stress ${Date.now()}`, content: "load-test post body" }),
      jsonHeaders,
    ),
  );
  check(create, { "create post: ok or rate-limited": (r) => r.status < 500 });

  // Read: list posts (per-post tag resolution — N+1 watch).
  const list = recordResponse(
    http.get(`${BASE_URL}/api/channels/${data.channelId}/posts`, h),
  );
  check(list, { "list posts: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
