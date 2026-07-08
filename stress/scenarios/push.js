// Mobile push-subscription stress scenario.
//
// Drives the subscription CRUD hot paths under concurrent load:
//   - POST   /api/me/push-subscriptions        (register/upsert a device)
//   - GET    /api/me/push-subscriptions         (list the caller's devices)
//   - DELETE /api/me/push-subscriptions/{id}    (unregister)
//
// The register→list→delete cycle exercises the ON CONFLICT (user, endpoint)
// upsert, the user-scoped list, and the user-scoped delete. No guild fixtures are
// needed — subscriptions hang off the authenticated user only. Each VU uses a
// per-iteration-unique endpoint so parallel VUs never collide on the (user,
// endpoint) unique key, and every row it creates it also deletes (no leak).
//
//   BASE_URL=http://localhost:8080 k6 run stress/scenarios/push.js
//   VUS=50 DURATION=1m BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/push.js
//
// NOTE: registration only stores the endpoint; no push is actually dispatched
// here (dispatch is triggered by DMs, covered by the smoke/message scenarios), so
// this never POSTs to an external distributor.

import http from "k6/http";
import { check, sleep } from "k6";
import { BASE_URL, baseOptions } from "../lib/config.js";
import { authenticate, authHeaders } from "../lib/auth.js";
import { recordResponse } from "../lib/metrics.js";

export const options = baseOptions;

export function setup() {
  const token = authenticate();
  return { token };
}

export default function (data) {
  const h = authHeaders(data.token);
  const jsonHeaders = { headers: { ...h.headers, "Content-Type": "application/json" } };

  // A per-iteration-unique endpoint so concurrent VUs never collide on the
  // (user, endpoint) unique key. Not a real distributor — never contacted here.
  const endpoint = `https://push.example.invalid/up?id=${__VU}-${__ITER}-${Date.now()}`;

  // Write: register (upsert) a device.
  const create = recordResponse(
    http.post(
      `${BASE_URL}/api/me/push-subscriptions`,
      JSON.stringify({ provider: "unifiedpush", endpoint, device_label: "k6" }),
      jsonHeaders,
    ),
  );
  check(create, { "register: ok or rate-limited": (r) => r.status < 500 });
  const subId = create.status < 300 ? create.json("id") : null;

  // Read: list the caller's devices.
  const list = recordResponse(
    http.get(`${BASE_URL}/api/me/push-subscriptions`, h),
  );
  check(list, { "list: ok or rate-limited": (r) => r.status < 500 });

  // Write: unregister the device we just created (no row leak).
  if (subId) {
    const del = recordResponse(
      http.del(`${BASE_URL}/api/me/push-subscriptions/${subId}`, null, h),
    );
    check(del, { "delete: ok or rate-limited": (r) => r.status < 500 });
  }

  sleep(1);
}

export function teardown(data) {
  // Best-effort sweep of any rows a mid-iteration failure left behind.
  const h = authHeaders(data.token);
  const list = http.get(`${BASE_URL}/api/me/push-subscriptions`, h);
  if (list.status === 200) {
    for (const sub of list.json()) {
      http.del(`${BASE_URL}/api/me/push-subscriptions/${sub.id}`, null, h);
    }
  }
}
