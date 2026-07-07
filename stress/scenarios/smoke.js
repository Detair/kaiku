// Smoke scenario — validates the harness itself, not a feature.
//
// Proves: config/target resolves, the auth helper obtains a token, requests
// fire, custom metrics record, and thresholds evaluate. Run this first after
// installing k6 and before trusting any feature scenario.
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/smoke.js
//
// Small, bounded load (safe against the beta). Override with VUS / DURATION.

import http from "k6/http";
import { check, sleep } from "k6";
import { BASE_URL, thresholds } from "../lib/config.js";
import { authenticate, authHeaders } from "../lib/auth.js";
import { recordResponse } from "../lib/metrics.js";

export const options = {
  vus: Number(__ENV.VUS || 5),
  duration: __ENV.DURATION || "20s",
  thresholds,
};

export function setup() {
  return { token: authenticate() };
}

export default function (data) {
  // Unauthenticated liveness endpoint.
  const health = recordResponse(http.get(`${BASE_URL}/health`));
  check(health, { "health: 200": (r) => r.status === 200 });

  // Authenticated read endpoint (accepts 429 under load — the rate limiter
  // working is not a smoke failure).
  const unread = recordResponse(
    http.get(`${BASE_URL}/api/me/unread`, authHeaders(data.token)),
  );
  check(unread, {
    "unread: 200 or 429": (r) => r.status === 200 || r.status === 429,
  });

  sleep(1);
}
