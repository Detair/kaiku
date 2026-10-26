const PROBE_ORIGIN = "https://kaiku.invalid";

/**
 * Validate a `returnUrl` query value: a same-origin path. Rejects "//host",
 * "/\host" and control characters (browsers strip those, turning
 * "/\t/host" into "//host"), and returns null for malformed escapes.
 */
export function safeReturnUrl(raw: string | string[] | undefined): string | null {
  const value = Array.isArray(raw) ? raw[0] : raw;
  if (!value) return null;
  let decoded: string;
  try {
    decoded = decodeURIComponent(value);
  } catch {
    return null;
  }
  // eslint-disable-next-line no-control-regex -- rejecting control characters is the point
  if (!/^\/(?![/\\])/.test(decoded) || /[\u0000-\u001f\u007f]/.test(decoded)) {
    return null;
  }
  try {
    const parsed = new URL(decoded, PROBE_ORIGIN);
    return parsed.origin === PROBE_ORIGIN
      ? `${parsed.pathname}${parsed.search}${parsed.hash}`
      : null;
  } catch {
    return null;
  }
}
