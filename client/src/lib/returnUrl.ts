/**
 * Validate a `returnUrl` query value: a same-origin path ("/..." but not
 * "//host" or "/\host", which browsers treat alike). Returns null for
 * anything else, including malformed escapes.
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
  return /^\/(?![/\\])/.test(decoded) ? decoded : null;
}
