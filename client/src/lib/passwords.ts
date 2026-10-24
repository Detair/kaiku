/**
 * Temporary password generation for admin-created accounts.
 */

// No look-alikes: 0 O o 1 l I are left out.
const ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnpqrstuvwxyz23456789";

/** Random temporary password; uses rejection sampling so there is no modulo bias. */
export function generateTemporaryPassword(length = 16): string {
  const limit = 256 - (256 % ALPHABET.length);
  let out = "";
  const buf = new Uint8Array(length * 2);
  while (out.length < length) {
    crypto.getRandomValues(buf);
    for (const byte of buf) {
      if (byte < limit) {
        out += ALPHABET[byte % ALPHABET.length];
        if (out.length === length) break;
      }
    }
  }
  return out;
}
