// Shared auth helper: obtain a bearer token for a dedicated load-test user.
//
// Call authenticate() from a scenario's setup() (runs once, before the VUs) so
// all VUs reuse one token and the login/register calls don't themselves hammer
// the auth rate limiter. The token is passed to the default function via the
// setup return value.

import http from "k6/http";
import { check } from "k6";
import { BASE_URL } from "./config.js";

const JSON_HEADERS = { "Content-Type": "application/json" };

// A dedicated throwaway account. Override for CI/beta with env vars.
const USER = __ENV.LOAD_USER || "k6_load_test";
const PASS = __ENV.LOAD_PASS || "k6-Load-Test-Pass-9271";

/**
 * Log in the load-test user, registering it first if it doesn't exist.
 * Returns a bearer access token. Throws (fails setup) if auth can't succeed.
 */
export function authenticate() {
  let res = http.post(
    `${BASE_URL}/auth/login`,
    JSON.stringify({ username: USER, password: PASS }),
    { headers: JSON_HEADERS },
  );

  if (res.status !== 200) {
    // Register (best-effort; open-registration servers only) then log in again.
    http.post(
      `${BASE_URL}/auth/register`,
      JSON.stringify({
        username: USER,
        password: PASS,
        display_name: "k6 Load Test",
      }),
      { headers: JSON_HEADERS },
    );
    res = http.post(
      `${BASE_URL}/auth/login`,
      JSON.stringify({ username: USER, password: PASS }),
      { headers: JSON_HEADERS },
    );
  }

  check(res, { "auth: login 200": (r) => r.status === 200 });
  const token = res.json("access_token");
  if (!token) {
    throw new Error(
      `authenticate(): no access_token (status ${res.status}). ` +
        `Is registration open, or the account already MFA-protected?`,
    );
  }
  return token;
}

/** Headers for an authenticated JSON request. */
export function authHeaders(token) {
  return {
    headers: { ...JSON_HEADERS, Authorization: `Bearer ${token}` },
  };
}
