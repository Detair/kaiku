// Membership-screening stress scenario.
//
// Drives the feature's hot paths under concurrent load:
//   - GET /api/guilds/{id}/screening        (read config + rules — polled by every
//                                             pending member while they read the gate)
//   - PUT /api/guilds/{id}/screening        (owner toggles screening / edits rules)
//
// The GET is the true hot path: a pending member's client fetches the rules to
// render the gate, and re-fetches on reconnect. setup() creates the guild and
// enables screening once, so per-VU iterations only touch the endpoints under
// test. The `accept` endpoint is not driven here — it is a one-shot per member
// (pending → active) and needs a distinct pending member per call, which a single
// shared VU token can't express; its correctness is covered by the integration
// tests.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/screening.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/screening.js

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
    JSON.stringify({ name: `screen-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  // Enable screening with rules so the GET path returns a populated config.
  const enable = http.put(
    `${BASE_URL}/api/guilds/${guildId}/screening`,
    JSON.stringify({ enabled: true, rules_md: "Be excellent to each other." }),
    jsonHeaders,
  );
  check(enable, { "setup: screening enabled": (r) => r.status < 300 });

  return { token, guildId };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Read: fetch the screening config + rules (the hot path).
  const get = recordResponse(
    http.get(`${BASE_URL}/api/guilds/${data.guildId}/screening`, h),
  );
  check(get, { "get: ok or rate-limited": (r) => r.status < 500 });

  // Write: owner edits the rules text (exercises the upsert).
  const put = recordResponse(
    http.put(
      `${BASE_URL}/api/guilds/${data.guildId}/screening`,
      JSON.stringify({ enabled: true, rules_md: `Rule revision ${__ITER}.` }),
      jsonHeaders,
    ),
  );
  check(put, { "put: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  // Guild delete cascades to the screening config and rules.
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
