/**
 * Extract an invite code from either a bare code or a link ending in
 * `/invite/<code>` (trailing slash and query string allowed).
 * Returns null if the input matches neither.
 */
export function extractInviteCode(input: string): string | null {
  const trimmed = input.trim();
  if (!trimmed) return null;

  const urlMatch = trimmed.match(/\/invite\/([A-Za-z0-9]+)\/?(?:[?#].*)?$/);
  if (urlMatch) return urlMatch[1];

  if (/^[A-Za-z0-9]{8,16}$/.test(trimmed)) return trimmed;

  return null;
}
