// Scheduled events stress scenario.
//
// Drives the events hot paths under load:
//   - POST /api/guilds/{id}/events            (create — MANAGE_EVENTS)
//   - GET  /api/guilds/{id}/events            (list upcoming, with aggregate
//                                              RSVP counts + my_response)
//   - PUT  /api/guilds/{id}/events/{eid}/rsvp (upsert RSVP)
//
// The list query aggregates RSVP counts per event with a GROUP BY — watch for
// latency climbing with event count (via the admin Command Center).
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/events.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/events.js

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
    JSON.stringify({ name: `event-stress-${Date.now()}` }),
    jsonHeaders,
  );
  check(guild, { "setup: guild created": (r) => r.status === 200 || r.status === 201 });
  const guildId = guild.json("id");

  // Seed one event to RSVP against.
  const startsAt = new Date(Date.now() + 3600_000).toISOString();
  const event = http.post(
    `${BASE_URL}/api/guilds/${guildId}/events`,
    JSON.stringify({ name: "Stress Event", starts_at: startsAt }),
    jsonHeaders,
  );
  check(event, { "setup: event created": (r) => r.status === 200 || r.status === 201 });

  return { token, guildId, eventId: event.json("id"), startsAt };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // Read: list upcoming events (aggregate RSVP counts).
  const list = recordResponse(
    http.get(`${BASE_URL}/api/guilds/${data.guildId}/events`, h),
  );
  check(list, { "list: ok or rate-limited": (r) => r.status < 500 });

  // Write: RSVP to the seeded event (upsert; idempotent).
  const rsvp = recordResponse(
    http.put(
      `${BASE_URL}/api/guilds/${data.guildId}/events/${data.eventId}/rsvp`,
      JSON.stringify({ response: "going" }),
      jsonHeaders,
    ),
  );
  check(rsvp, { "rsvp: ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown(data) {
  http.del(`${BASE_URL}/api/guilds/${data.guildId}`, null, authHeaders(data.token));
}
