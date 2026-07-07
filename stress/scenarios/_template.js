// Per-feature scenario TEMPLATE. Copy to `<feature>.js` and fill in.
//
// Each gap feature adds one scenario here that exercises its new endpoints under
// concurrent load. Keep to the shared config/metrics/auth helpers so every
// scenario reports the same thresholds and the same 5xx-vs-429 distinction.
//
//   cp stress/scenarios/_template.js stress/scenarios/reaction-roles.js
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/reaction-roles.js
//
// Checklist for a feature scenario:
//   - Drive the feature's hot path(s): the write endpoint(s) + a read/list.
//   - Use setup() for one-time fixtures (a guild, a channel, a message, a role)
//     so per-VU iterations only hit the endpoint under test.
//   - Assert correctness under load (status + a body check), not just latency.
//   - Watch the admin Command Center (CPU / memory / p95 / error rate) during
//     the run; a query-count/N+1 regression shows up as latency growth with VUs.

import http from "k6/http";
import { check, sleep } from "k6";
import { BASE_URL, baseOptions } from "../lib/config.js";
import { authenticate, authHeaders } from "../lib/auth.js";
import { recordResponse } from "../lib/metrics.js";

export const options = baseOptions;

export function setup() {
  const token = authenticate();
  // TODO: create the fixtures this feature needs (guild/channel/message/role…)
  // and return their ids alongside the token.
  return { token };
}

export default function (data) {
  const h = authHeaders(data.token);

  // TODO: replace with the feature's endpoint(s).
  const res = recordResponse(http.get(`${BASE_URL}/api/me/unread`, h));
  check(res, { "ok or rate-limited": (r) => r.status < 500 });

  sleep(1);
}

export function teardown() {
  // TODO: delete any fixtures created in setup() (guild delete cascades most).
}
