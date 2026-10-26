/**
 * Server error codes are compared case-insensitively: the server is moving
 * every code to UPPER_SNAKE, and some arrive in lowercase until it has. Route
 * every code comparison through here rather than `=== "some_code"`.
 */

/** Trimmed, upper-cased code; `null` for anything that is not a non-empty string. */
export function normalizeCode(code: unknown): string | null {
  if (typeof code !== "string") return null;
  const normalized = code.trim().toUpperCase();
  return normalized === "" ? null : normalized;
}

/** True when `code` is the server code `expected`, ignoring case and padding. */
export function codeIs(code: unknown, expected: string): boolean {
  const actual = normalizeCode(code);
  return actual !== null && actual === normalizeCode(expected);
}

/** True when free text (an error message or response body) mentions `code`. */
export function messageHasCode(text: unknown, code: string): boolean {
  const needle = normalizeCode(code);
  return typeof text === "string" && needle !== null && text.toUpperCase().includes(needle);
}
