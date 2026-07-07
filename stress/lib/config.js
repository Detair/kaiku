// Shared k6 configuration for the Kaiku load/stress harness.
//
// Target is chosen by env var so the same scenarios run against a local server
// or the beta:  BASE_URL=https://kaiku.pmind.de k6 run stress/scenarios/smoke.js
//
// See stress/README.md for usage and the rate-limit / beta caveats.

export const BASE_URL = __ENV.BASE_URL || "http://localhost:8080";

// Load shape, overridable per run.  Kept small by default so an accidental beta
// run stays bounded.
export const VUS = Number(__ENV.VUS || 10);
export const DURATION = __ENV.DURATION || "30s";

// Pass/fail gates. The real failure signal is server errors (5xx) and latency;
// a 429 is the rate limiter working *correctly* under stress, so it is tracked
// separately (see lib/metrics.js) and does NOT fail the run. This is why we
// gate on `server_errors` rather than k6's built-in `http_req_failed` (which
// counts every 4xx, including expected 429s, as a failure).
export const thresholds = {
  server_errors: ["rate<0.01"], // < 1% of requests return 5xx
  http_req_duration: ["p(95)<800"], // p95 latency under 800ms
  checks: ["rate>0.95"], // > 95% of scenario assertions pass
};

export const baseOptions = {
  vus: VUS,
  duration: DURATION,
  thresholds,
  // Abort a run early if the error gate is already blown — fail fast.
  abortOnFail: true,
};
