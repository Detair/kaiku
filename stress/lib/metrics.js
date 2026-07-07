// Custom k6 metrics that separate real failures from expected back-pressure.
//
// Under stress the server *should* start returning 429 (rate limited) — that is
// correct behavior, not a failure. So we track:
//   - server_errors : rate of 5xx responses  (the real failure gate)
//   - rate_limited  : rate of 429 responses  (informational; expected under load)
// Call recordResponse(res) after every request in a scenario.

import { Rate } from "k6/metrics";

export const serverErrors = new Rate("server_errors");
export const rateLimited = new Rate("rate_limited");

export function recordResponse(res) {
  serverErrors.add(res.status >= 500);
  rateLimited.add(res.status === 429);
  return res;
}
