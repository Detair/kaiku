/**
 * Build the `kaiku://qr/login` deep link that the mobile app scans.
 * Format is parsed by the Android client; do not change it.
 *
 * @throws Error("QR_NO_SERVER") unless `serverUrl` is an absolute http(s) URL.
 */
export function buildQrLoginUri(serverUrl: string, token: string): string {
  let protocol = "";
  try {
    protocol = new URL(serverUrl).protocol;
  } catch {
    // fall through: not an absolute URL
  }
  if (protocol !== "http:" && protocol !== "https:") {
    throw new Error("QR_NO_SERVER");
  }
  return `kaiku://qr/login?server=${encodeURIComponent(serverUrl)}&token=${encodeURIComponent(token)}`;
}
